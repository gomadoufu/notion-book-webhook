# Notion Book Author Webhook

Notionデータベースの本のタイトルから自動的に著者情報を取得して入力します。

## 機能

- Google Books APIで書籍情報を検索
- Author Nameプロパティを自動更新
- 5分ごとにデータベースをポーリング

## 環境変数

- `NOTION_TOKEN`: Notion Integration Token
- `NOTION_DATABASE_ID`: 対象のDatabase ID
- `PORT`: サーバーポート（Renderが自動設定）

## デプロイ

1. Renderでアカウント作成
2. New Web Service
3. Connect Repository
4. 環境変数を設定
5. Deploy

## ローカル実行
```bash
cp .env.example .env
# .envにトークンを記入
cargo run
```
