use reqwest::Error;
use axum::{error_handling::HandleErrorLayer, routing::get, BoxError, Router, body::Body};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use axum::extract::ConnectInfo;
use serde_json::Value;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use axum_core::*;
use cached::proc_macro::cached;
use cached::SizedCache;
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};
use tower_governor::key_extractor::SmartIpKeyExtractor;
use tracing_subscriber::fmt::format;

static SEARCH_RADIUS: u32 = 1000000000; // metres
static PORT : u16 = 80;

#[tokio::main]
async fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(2)
            .burst_size(20)
            .finish()
            .unwrap(),
    );
    let governor_limiter = governor_conf.limiter().clone();
    let interval = Duration::from_secs(60);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval);
            tracing::info!("rate limiting storage size: {}", governor_limiter.len());
            governor_limiter.retain_recent();
        }
    });

    let app = Router::new()
        .route("/", get(root)).layer(GovernorLayer {
        config: governor_conf,
    });

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", PORT.to_string())).await.unwrap();
    println!("listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}

async fn root(addr: ConnectInfo<SocketAddr>) -> String {
    let ip = addr.ip().to_string();
    println!("{}", ip);

    get_airport(ip.to_string()).await.unwrap()

}





#[cached(
    ty = "cached::TimedCache<String, String>",
    result = true,
    create = "{ cached::TimedCache::with_lifespan(604800) }",
    key = "String",
    convert = r#"{ address.clone() }"#
)]
async fn get_airport(mut address: String) -> Result<String, Error> {
    // let ip_address = address;
    if address == "127.0.0.1" {
        address = "".to_string(); // ip API defaults to own IP when empty
    }
    let ip_url = format!("https://freeipapi.com/api/json/{}", address);
    let ip_response = reqwest::get(&ip_url).await?.text().await?;
    let ip_parsed: Value = serde_json::from_str(&ip_response).unwrap();
    let longitude = ip_parsed.get("longitude").unwrap();
    let latitude = ip_parsed.get("latitude").unwrap();

    let port_url = format!("https://port-api.com/airport/near/{}/{}?search_radius={}&airport_size=large_airport", longitude, latitude, SEARCH_RADIUS);
    let port_response = reqwest::get(&port_url).await?.text().await?;
    let port_parsed: Value = serde_json::from_str(&port_response).unwrap();
    let features: &Value = port_parsed.get("features").unwrap();

    let mut iata = "NONE".to_string();
    let mut name = "NONE".to_string();
    let mut distance = -1.00;
    for feature in features.as_array().unwrap() {
        let properties = feature.get("properties").unwrap().clone();
        let newDistance = properties.get("distance").unwrap().as_f64().unwrap();
        if distance == -1.00 || newDistance < distance {
            distance = newDistance;
            iata = properties.get("iata").unwrap().as_str().unwrap().parse().unwrap();
            name = properties.get("name").unwrap().as_str().unwrap().parse().unwrap();
        }
    }
    println!("-------------------------");
    println!("{}", address);
    println!("Closest large airport: {}", iata);
    println!("{} metres away", distance);
    println!("-------------------------");

    Ok(format!("{}: {}", name, iata))
}