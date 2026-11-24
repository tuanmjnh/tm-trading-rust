# TM Trading Rust — Unified Desktop Dashboard Architecture

## 1. Core Design Concept

The TM Trading Desktop UI is redesigned from a multi-tab navigation model into a **single, information-dense Bento/Split Unified Dashboard**.

### Key Tenets:
1. **Zero Tab Context-Switching**: All primary operational metrics, service controls, live market data, research signals, news headlines, and notification statuses are visible on one cohesive screen.
2. **Dense Bento Grid Layout**: Proportional, high-contrast cards providing immediate situational awareness at a glance.
3. **Modal & Slideover Overlays**: In-depth investigations, log viewers, AI provider CRUD forms, Telegram channel wizards, and system diagnostics open as non-disruptive modals or slideovers without leaving the dashboard context.
4. **Bilingual Localization**: Full English (`en`) and Vietnamese (`vi`) support with dynamic runtime toggling.

---

## 2. Dashboard Layout Anatomy

```
+---------------------------------------------------------------------------------------+
|  HEADER: App Title | System Status Pill | Mongo Status | Refresh Timer | Lang | Config |
+---------------------------------------------------------------------------------------+
|  KPI SUMMARY CARDS (7 Bento Tiles):                                                  |
|  [ Services: 6/6 ] [ Market: 3 Syms ] [ Signals: 24h ] [ News: 48 ] [ AI Jobs ] ...  |
+---------------------------------------------------------------------------------------+
|  SPLIT SECTION 1: Service Control Center (Left) & Market Snapshot (Right)            |
|  - Table of 6 Workers (State, PID, Health, Actions)  | - Live Tickers, 24h Chg, High  |
+---------------------------------------------------------------------------------------+
|  SPLIT SECTION 2: Research Signals (Left) & News / AI Briefings (Right)              |
|  - VSA / Trend / Price-Action Signals with Badges    | - Latest Articles & AI Briefs  |
+---------------------------------------------------------------------------------------+
|  FOOTER SECTION: Outbox Delivery Queue & System Health Bar                            |
+---------------------------------------------------------------------------------------+
```

---

## 3. Detail Views via Overlays

| Interaction | Trigger | Overlay Type | Contents |
|---|---|---|---|
| **Service Details & Logs** | Service Control Center row action | `Slideover` | Full process identity, readiness rule evaluations, restart counts, live supervisor log stream. |
| **Research Signal Details** | Clicking any signal card | `Slideover` | Timeframe, bar index, volume spread analysis metadata, confidence score, price charts. |
| **News Article Detail** | Clicking any news card | `Slideover` | Source metadata, full excerpt, tags, original URL link, sentiment score. |
| **AI Briefing Exploration** | Clicking an AI briefing card | `Slideover` | Synthesis narrative, evidence citations, market bias, uncertainties. |
| **AI Provider / Route CRUD** | Header Settings dropdown or KPI | `Modal` | Manage provider profiles (Gemini, OpenRouter, OpenAI), models, and route mappings. |
| **Telegram Setup Wizard** | Notification status tile or Settings | `Modal` | Bot token configuration, chat ID testing, message delivery audit. |
| **Add Watchlist Symbol** | Market Snapshot action | `Modal` | Fast symbol addition form with validation. |
