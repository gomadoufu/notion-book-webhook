use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

// Notionからのwebhookペイロード
#[derive(Debug, Deserialize)]
struct NotionWebhook {
    page_id: String,
    properties: Properties,
}

#[derive(Debug, Deserialize)]
struct Properties {
    #[serde(rename = "Title")]
    title: TitleProperty,
}

#[derive(Debug, Deserialize)]
struct TitleProperty {
    title: Vec<TitleContent>,
}

#[derive(Debug, Deserialize)]
struct TitleContent {
    plain_text: String,
}

// Google Books APIレスポンス
#[derive(Debug, Deserialize)]
struct BooksResponse {
    #[serde(default)]
    items: Vec<BookItem>,
}

#[derive(Debug, Deserialize)]
struct BookItem {
    #[serde(rename = "volumeInfo")]
    volume_info: VolumeInfo,
}

#[derive(Debug, Deserialize)]
struct VolumeInfo {
    #[serde(default)]
    authors: Vec<String>,
}

// アプリケーションステート
#[derive(Clone)]
struct AppState {
    notion_token: String,
    http_client: reqwest::Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 環境変数読み込み
    dotenvy::dotenv().ok();
    
    let notion_token = std::env::var("NOTION_TOKEN")
        .expect("NOTION_TOKEN must be set");
    
    // アプリケーションステート
    let state = AppState {
        notion_token,
        http_client: reqwest::Client::new(),
    };
    
    // ルーター構築
    let app = Router::new()
        .route("/webhook", post(handle_webhook))
        .route("/health", axum::routing::get(|| async { "OK" }))
        .with_state(state);
    
    // ポート設定（Renderは環境変数PORTを設定）
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid u16");
    
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    println!("🚀 Server running on {}", addr);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

// Webhookハンドラー
async fn handle_webhook(
    State(state): State<AppState>,
    Json(payload): Json<NotionWebhook>,
) -> impl IntoResponse {
    println!("📥 Received webhook for page: {}", payload.page_id);
    
    match process_webhook(state, payload).await {
        Ok(_) => {
            println!("✅ Successfully processed webhook");
            (StatusCode::OK, Json(json!({"status": "success"})))
        }
        Err(e) => {
            eprintln!("❌ Error processing webhook: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": e.to_string()})),
            )
        }
    }
}

async fn process_webhook(
    state: AppState,
    payload: NotionWebhook,
) -> anyhow::Result<()> {
    // タイトル取得
    let title = payload
        .properties
        .title
        .title
        .first()
        .map(|t| t.plain_text.clone())
        .unwrap_or_default();
    
    if title.is_empty() {
        println!("⚠️  No title found, skipping");
        return Ok(());
    }
    
    println!("📚 Processing book: {}", title);
    
    // Google Books APIで検索
    let author = fetch_book_author(&state.http_client, &title).await?;
    
    if let Some(author) = author {
        println!("✍️  Found author: {}", author);
        
        // Notionページ更新
        update_notion_page(&state, &payload.page_id, &author).await?;
        
        println!("💾 Updated Notion page with author info");
    } else {
        println!("⚠️  No author found for: {}", title);
    }
    
    Ok(())
}

// Google Books APIから著者情報取得
async fn fetch_book_author(
    client: &reqwest::Client,
    title: &str,
) -> anyhow::Result<Option<String>> {
    let url = "https://www.googleapis.com/books/v1/volumes";
    
    let response = client
        .get(url)
        .query(&[
            ("q", format!("intitle:{}", title)),
            ("maxResults", "1".to_string()),
        ])
        .send()
        .await?;
    
    let books: BooksResponse = response.json().await?;
    
    Ok(books.items.first().and_then(|item| {
        if item.volume_info.authors.is_empty() {
            None
        } else {
            Some(item.volume_info.authors.join(", "))
        }
    }))
}

// Notionページ更新（reqwestで直接API呼び出し）
async fn update_notion_page(
    state: &AppState,
    page_id: &str,
    author: &str,
) -> anyhow::Result<()> {
    let url = format!("https://api.notion.com/v1/pages/{}", page_id);
    
    let body = json!({
        "properties": {
            "Author": {
                "rich_text": [
                    {
                        "type": "text",
                        "text": {
                            "content": author
                        }
                    }
                ]
            }
        }
    });
    
    let response = state
        .http_client
        .patch(&url)
        .header("Authorization", format!("Bearer {}", state.notion_token))
        .header("Notion-Version", "2022-06-28")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error_text = response.text().await?;
        anyhow::bail!("Failed to update Notion page: {}", error_text);
    }
    
    Ok(())
}
