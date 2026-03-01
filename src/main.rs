use axum::{routing::post, Router, response::IntoResponse, http::StatusCode};
use chromiumoxide::{Browser, BrowserConfig};
use dotenvy::dotenv;
use futures::StreamExt;
use std::{env, fs};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    // Load .env variables
    dotenv().ok();
    
    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    
    let app = Router::new()
        .route("/automate", post(run_automation));

    println!("Starting server on http://{}", addr);
    println!("Send a POST request to /automate to begin shopping.");
    
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn run_automation() -> impl IntoResponse {
    let username = env::var("SUPERMARKET_USERNAME").unwrap_or_default();
    let password = env::var("SUPERMARKET_PASSWORD").unwrap_or_default();
    let url = env::var("SUPERMARKET_URL").unwrap_or_else(|_| "https://www.planetorganic.com".to_string());

    if username.is_empty() || password.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing credentials in .env file");
    }

    // Load groceries from JSON
    let groceries_file = fs::read_to_string("groceries.json").unwrap_or_else(|_| "[]".to_string());
    let groceries: Vec<String> = serde_json::from_str(&groceries_file).unwrap_or_default();

    if groceries.is_empty() {
        return (StatusCode::BAD_REQUEST, "Groceries list is empty or groceries.json is missing");
    }

    // Spawning the automation logic
    match perform_shopping(url, username, password, groceries).await {
        Ok(_) => (StatusCode::OK, "Automation completed successfully. Browser left open for 10 minutes for manual checkout."),
        Err(e) => {
            eprintln!("Error during automation: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "An error occurred during browser automation.")
        }
    }
}

async fn perform_shopping(
    url: String, 
    username: String, 
    password: String, 
    groceries: Vec<String>
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Launch browser visibly (with head) for manual checkout inspection
    let (mut browser, mut handler) = Browser::launch(
        BrowserConfig::builder()
            .with_head()
            .window_size(1280, 800)
            .build()?,
    )
    .await?;

    let handle = tokio::task::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
    });

    let page = browser.new_page(&url).await?;
    println!("Navigated to {}", url);
    page.wait_for_navigation().await?;
    sleep(Duration::from_secs(3)).await;

    // NOTE: The CSS selectors below are generic examples. 
    // They will need to be adjusted based on the specific DOM of the target supermarket.

    // 2. Login
    println!("Attempting login...");
    if let Ok(login_link) = page.find_element("a[href*='login'], button[class*='login']").await {
        let _ = login_link.click().await;
        sleep(Duration::from_secs(3)).await;
        
        if let Ok(user_field) = page.find_element("input[type='email'], input[name*='user']").await {
            let _ = user_field.click().await?.type_str(&username).await;
        }
        if let Ok(pass_field) = page.find_element("input[type='password']").await {
            let _ = pass_field.click().await?.type_str(&password).await;
        }
        if let Ok(submit_btn) = page.find_element("button[type='submit']").await {
            let _ = submit_btn.click().await;
        }
        sleep(Duration::from_secs(5)).await;
    } else {
        println!("Login link not found, proceeding anyway...");
    }

    // 3. Search and add groceries to basket
    for item in groceries {
        println!("Searching for: {}", item);
        if let Ok(search_bar) = page.find_element("input[type='search'], input[name='q'], input[id*='search']").await {
            let _ = search_bar.click().await;
            
            // Clear search bar using JS eval
            let _ = page.evaluate("document.querySelector('input[type=\"search\"], input[name=\"q\"], input[id*=\"search\"]').value = ''").await;
            
            let _ = search_bar.type_str(&item).await?.press_key("Enter").await;
            sleep(Duration::from_secs(4)).await;

            if let Ok(add_btn) = page.find_element("button[class*='add'], button[aria-label*='Add']").await {
                println!("Adding {} to basket...", item);
                let _ = add_btn.click().await;
                sleep(Duration::from_secs(2)).await;
            } else {
                println!("Could not find 'Add to basket' button for {}", item);
            }
        } else {
            println!("Search bar not found!");
        }
    }

    // 4. Identify the Delivery Slot page
    println!("Navigating to delivery slots...");
    if let Ok(delivery_link) = page.find_element("a[href*='delivery'], a[href*='slot'], button[class*='slot']").await {
        let _ = delivery_link.click().await;
        sleep(Duration::from_secs(4)).await;

        if let Ok(slot_btn) = page.find_element("button[class*='available-slot'], button[data-test-id*='slot']").await {
            println!("Selecting delivery slot...");
            let _ = slot_btn.click().await;
            sleep(Duration::from_secs(2)).await;
        } else {
            println!("No available delivery slots found.");
        }
    } else {
        println!("Delivery slot link not found.");
    }

    println!("Shopping automation complete! Leaving the browser open for 10 minutes for manual checkout.");
    sleep(Duration::from_secs(600)).await;

    browser.close().await?;
    let _ = handle.await;

    Ok(())
}
