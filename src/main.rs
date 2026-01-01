// src/main.rs（完全版）
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use tokio::time::{sleep, Duration};

// Notion Database Query APIレスポンス
#[derive(Debug, Deserialize)]
struct QueryDatabaseResponse {
    results: Vec<NotionPage>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct NotionPage {
    id: String,
    properties: PageProperties,
}

#[derive(Debug, Deserialize, Clone)]
struct PageProperties {
    #[serde(rename = "Title")]
    title: Option<TitleProperty>,
    #[serde(rename = "Author")]
    author: Option<RichTextProperty>,
}

#[derive(Debug, Deserialize, Clone)]
struct TitleProperty {
    title: Vec<TitleContent>,
}

#[derive(Debug, Deserialize, Clone)]
struct RichTextProperty {
    rich_text: Vec<RichTextContent>,
}

#[derive(Debug, Deserialize, Clone)]
struct TitleContent {
    plain_text: String,
}

#[derive(Debug, Deserialize, Clone)]
struct RichTextContent {
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
    database_id: String,
    http_client: reqwest::Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 環境変数読み込み
    dotenvy::dotenv().ok();
    
    let notion_token = std::env::var("NOTION_TOKEN")
        .expect("NOTION_TOKEN must be set");
    
    let database_id = std::env::var("NOTION_DATABASE_ID")
        .expect("NOTION_DATABASE_ID must be set");
    
    println!("🔧 Using Database ID: {}", database_id);
    
    // アプリケーションステート
    let state = AppState {
        notion_token,
        database_id: database_id.clone(),
        http_client: reqwest::Client::new(),
    };
    
    // ポーリングタスクを起動
    let poll_state = state.clone();
    tokio::spawn(async move {
        poll_database(poll_state).await;
    });
    
    // ルーター構築（ヘルスチェック＋手動トリガー用）
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/trigger", post(manual_trigger))
        .with_state(state);
    
    // ポート設定
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid u16");
    
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    println!("🚀 Server running on {}", addr);
    println!("📊 Polling database every 300 seconds...");
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

// ヘルスチェック
async fn health_check() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

// 手動トリガー（テスト用）
async fn manual_trigger(State(state): State<AppState>) -> impl IntoResponse {
    println!("🔄 Manual trigger requested");
    
    match process_database(&state).await {
        Ok(count) => {
            let message = format!("Processed {} pages", count);
            println!("✅ {}", message);
            (StatusCode::OK, Json(json!({"status": "success", "message": message})))
        }
        Err(e) => {
            eprintln!("❌ Error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": e.to_string()})),
            )
        }
    }
}

// データベースポーリング
async fn poll_database(state: AppState) {
    let mut processed_pages: HashSet<String> = HashSet::new();
    
    loop {
        println!("\n🔍 Polling database...");
        
        match process_database(&state).await {
            Ok(count) => {
                if count > 0 {
                    println!("✅ Processed {} new pages", count);
                } else {
                    println!("ℹ️  No new pages to process");
                }
            }
            Err(e) => eprintln!("❌ Error polling database: {}", e),
        }
        
        // 60秒待機
        sleep(Duration::from_secs(300)).await;
    }
}

// データベース処理
async fn process_database(state: &AppState) -> anyhow::Result<usize> {
    // Authorが空のページを取得
    let pages = query_database_empty_authors(state).await?;
    
    let mut processed_count = 0;
    
    for page in pages {
        if let Err(e) = process_page(state, &page).await {
            eprintln!("❌ Error processing page {}: {}", page.id, e);
        } else {
            processed_count += 1;
        }
    }
    
    Ok(processed_count)
}

// Authorが空のページをクエリ
async fn query_database_empty_authors(
    state: &AppState,
) -> anyhow::Result<Vec<NotionPage>> {
    let url = format!(
        "https://api.notion.com/v1/databases/{}/query",
        state.database_id
    );
    
    let body = json!({
        "filter": {
            "and": [
                {
                    "property": "Title",
                    "title": {
                        "is_not_empty": true
                    }
                },
                {
                    "property": "Author",
                    "rich_text": {
                        "is_empty": true
                    }
                }
            ]
        }
    });
    
    let response = state
        .http_client
        .post(&url)
        .header("Authorization", format!("Bearer {}", state.notion_token))
        .header("Notion-Version", "2022-06-28")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error_text = response.text().await?;
        anyhow::bail!("Failed to query database: {}", error_text);
    }
    
    let query_response: QueryDatabaseResponse = response.json().await?;
    
    println!("📋 Found {} pages with empty Author", query_response.results.len());
    
    Ok(query_response.results)
}

// 個別ページ処理
async fn process_page(state: &AppState, page: &NotionPage) -> anyhow::Result<()> {
    // タイトル取得
    let title = page
        .properties
        .title
        .as_ref()
        .and_then(|t| t.title.first())
        .map(|t| t.plain_text.clone())
        .unwrap_or_default();
    
    if title.is_empty() {
        return Ok(());
    }
    
    println!("📚 Processing: {}", title);
    
    // Google Books APIで検索
    let author = fetch_book_author(&state.http_client, &title).await?;
    
    if let Some(author) = author {
        println!("  ✍️  Found author: {}", author);
        
        // Notionページ更新
        update_notion_page(state, &page.id, &author).await?;
        
        println!("  💾 Updated!");
    } else {
        println!("  ⚠️  No author found");
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

// Notionページ更新
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
