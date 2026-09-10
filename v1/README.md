**Languages:** [English](README.md) | [中文](README.zh-CN.md)

<div align="center">
  <h1>📊 RdataStation</h1>
  <p><em>Analysis begins where the query ends.</em></p>
  <p><strong>Every query has a purpose. Every analysis, a conscience.</strong></p>
</div>

---

> If you've ever found yourself bouncing between SQL query results and Excel, you're not alone. This is an attempt to solve that.

[![Rust](https://img.shields.io/badge/Rust-000000?logo=rust)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-FFC131?logo=tauri)](https://tauri.app)
[![Vue3](https://img.shields.io/badge/Vue-3.x-4FC08D?logo=vue.js)](https://vuejs.org)
[![DuckDB](https://img.shields.io/badge/DuckDB-built--in-FFF000)](https://duckdb.org)
[![License](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue)](./LICENSE)

> **Note on language:** This README was originally written in Chinese and translated with the help of AI. English is not my native language, so please excuse any awkward phrasing. Corrections and suggestions are always welcome.

---

## 📖 What is RdataStation?

RdataStation is a **local-first, offline-first** database desktop tool.

There are already many mature database tools out there. DBeaver is a treasure of the open-source community. DataGrip sets the benchmark for SQL editing experience. Navicat is known for its stability and ease of use. We have deep respect for all of them.

RdataStation was not created to replace these great products. It was created to try to answer a question that, for historical and architectural reasons, they haven't focused on yet:

**After you run a SQL query and get back hundreds of thousands of rows — what do you do next?**

Export to Excel? Import into Python and write pandas? Paste into Jupyter to plot? This workflow is not only fragmented — it breaks down entirely when dealing with large datasets.

**Our answer: let analysis happen after the query, right inside the tool.**

---

## 👀 A familiar scenario

You write a SQL query. It returns 200,000 rows.

You want to group by region and summarize. You export to CSV. Excel struggles to open it. You switch to Python, import pandas, load the CSV, write a `groupby`. The fan on your laptop starts spinning.

Meanwhile, all you wanted to do was ask a few follow-up questions about your query results.

**In a test with 500,000 rows:** Excel took 30+ seconds to load and froze during filtering. Python + pandas took ~8 seconds to load and ~3 seconds to group. RdataStation + DuckDB returned the same grouping result in roughly 0.3 seconds, directly on the query result panel. _(Benchmark on a standard laptop; your results may vary.)_

**With RdataStation:** click the **"DuckDB Analysis"** button on the result panel. The 200,000 rows become a local temporary table inside DuckDB. Type in your aggregation SQL. Results come back in milliseconds. No export. No Excel. No Python. The entire chain of questioning happens inside one tool.

- The temporary table is **session-level** — it disappears when you close the panel.
- If you want to keep the result, click **"Persist"** — it saves into the project-level DuckDB.
- If the entire team needs it, click **"Promote to Shared Asset"** — it goes into the app-level DuckDB, accessible to all projects.

---

## 🎯 What it is — and what it isn't

RdataStation focuses on the space **after the query and before visualization**.

Connection, querying, and management are the **foundation**. Secondary analysis, federated queries, and data profiling are the **extension**. Charts, dashboards, and reports — those we leave to the plugin ecosystem.

We're not trying to build a "database tool that does everything." We're trying to build a bridge between your query results and your analysis conclusions.

---

## 💡 The design philosophy

### 🏠 Local-first. Your data stays with you.

All connection info, query history, and analysis cache live only on your computer. No telemetry. No cloud sync. No "please log in to continue." Your database passwords, your queries, your results — they never leave your machine.

### 🧩 SQL is the entrypoint, but not necessarily the destination

RdataStation uses SQL as the entry point for analysis — connecting, querying, secondary analysis, federated queries, data profiling are all done through SQL. When results need further processing, plugins can take over: Python for visualization, Extism plugins for data masking or format conversion. SQL handles the "what to analyze"; plugins handle the "what to produce." Both work together in the same workbench.

---

## ⚙️ How it works: the architecture behind it

<img width="5044" height="4164" alt="image" src="https://github.com/user-attachments/assets/3e8be19f-b95d-4a16-89a5-81b022c22e76" />

### 🧠 DuckDB as the analysis engine

DuckDB is not "just another connectable data source" in RdataStation — it's embedded into the core.

- **Secondary analysis**: any query result set can be transferred into a local DuckDB table with one click. `GROUP BY`, window functions, subqueries — all happen in milliseconds, locally.
  - _When you need this:_ You just ran a query. Instead of exporting to Excel and waiting, you keep exploring right inside the tool.
- **Federated queries**: join tables across MySQL, PostgreSQL, and local CSV files in a single SQL statement.
  - _When you need this:_ Your user data is in one database, orders in another, and a mapping file is on your desktop. You join them with one query.
- **Data profiling**: click any column in the result grid. A side panel instantly shows the column's profile — mean, min, max, median for numeric columns; distinct count, null ratio, top-10 frequency for text columns. All powered by DuckDB, locally, without touching the remote database.
  - _When you need this:_ Before analyzing, you want to know what the data looks like — are there empty values? What's the distribution? You get answers in one click.
- **Three result-set modes**: instant filtering (frontend, zero latency), SQL filtering (re-executes the query with appended WHERE clause, like DBeaver), and DuckDB deep analysis (aggregation, window functions, federated queries on the local DuckDB copy).
- **Version snapshots**: when a shared dimension table is updated, the old version is preserved as a snapshot rather than being overwritten. Each project locks to a specific version. Analysis results remain reproducible.
  - _When you need this:_ You wrote a quarterly report in January based on the "product categories v1" table. In March, someone updates it to v2. Without version snapshots, your January report silently changes. With snapshots, it stays exactly as it was.

### 🏗️ Two-tier architecture: App-level + Project-level

RdataStation organizes data work into two tiers, each with its own SQLite persistence and DuckDB engine.

- **Project-level (personal analysis sandbox)**: each project is physically isolated with its own SQLite and DuckDB files. Projects cannot see each other's data. Your analysis process, intermediate results, and exploratory queries stay independent.
- **App-level (shared service layer)**: stores shared connection templates, master data, dimension tables, and analysis assets. All projects can reference these.

When the same data is used repeatedly across multiple projects — like a supplier master table needed by marketing, inventory, and procurement analysis — the user can manually **promote** it to the app level. Clean it once, use it everywhere. When the master data changes, a new version snapshot is created. Each project decides independently when to sync to the new version or lock to an older one.

### 🧩 Plugin system (implemented)

RdataStation provides two extension channels:

- **Extism lightweight plugins (WASM sandbox)**: community-built plugins for SQL formatting, data masking, code generation, and analysis templates. Sandboxed for safety. Support for Rust, Go, Python, and JavaScript.
- **Sidecar for heavy dependencies**: independent processes for JDBC bridging (requires JVM) and Python analysis environments (requires pandas). Started on demand, never running when idle. Crash in a Sidecar process never affects the main application.

---

## 🚧 Current state

🟢 **Active development — v0.5.x (Alpha)**

The core architecture is solid, and all major modules are functional. The project is now in the refinement and optimization phase across frontend and backend.

### ✅ Implemented

#### Database Connectivity

- [x] **MySQL** — native connection via sqlx, SSH tunnel support, SSL/TLS
- [x] **PostgreSQL** — native connection via sqlx, SSH tunnel support, SSL/TLS
- [x] **SQLite** — native connection via rusqlite, bundled support
- [x] **DuckDB** — embedded analysis engine, native driver
- [x] **Connection profiles** — global + project-level, 25-column schema alignment
- [x] **Authentication** — AES-256-GCM encrypted credentials, 7 auth methods (password, LDAP, Kerberos, OAuth2, OS auth, trust, SSH)
- [x] **Network tunnels** — SSH (russh), SSL/TLS (native-tls), proxy support

#### SQL Editor & Query Execution

- [x] **CodeMirror 6** — syntax highlighting, SQL language support, error diagnostics
- [x] **Multi-mode execution** — run selected, run all, run current statement
- [x] **DuckDB acceleration** — one-click local analysis of query results
- [x] **Smart execution** — row-count estimation before execution
- [x] **Batch execution** — multiple statements with independent result sets
- [x] **Query cancellation** — abort in-progress queries
- [x] **SQL history** — backend-persisted execution history
- [x] **SQL parser** — sqlglot-rust integration for statement parsing and routing
- [x] **Scratchpad editor** — unsaved file support, draft persistence, crash recovery
- [x] **File CRUD** — dirty-close interception, external change detection, cross-window conflict resolution

#### Data Analysis & DuckDB

- [x] **Secondary analysis** — query results → local DuckDB temp tables
- [x] **Federated queries** — cross-database JOINs (MySQL + PostgreSQL + CSV)
- [x] **Data profiling** — column-level statistics (mean, median, min, max, null ratio, frequency)
- [x] **Version snapshots** — immutable dimension table versions
- [x] **Full-text search** — DuckDB FTS integration
- [x] **Import/Export** — CSV, Parquet, JSON, Arrow
- [x] **DuckDB extensions** — pluggable extension loading
- [x] **Query explain** — DuckDB EXPLAIN ANALYZE support

#### Project & Asset Management

- [x] **Two-tier architecture** — app-level shared services + project-level isolation
- [x] **Analytics Resource Manager** — folder hierarchy, versioning, tagging, recycle bin
- [x] **Project CRUD** — create, load, save, delete projects
- [x] **Metadata browser** — schema tree, lazy-loaded children, database object navigation
- [x] **SQL templates** — reusable parameterized queries

#### Plugin System

- [x] **Extism WASM runtime** — sandboxed plugin execution (Rust, Go, Python, JS)
- [x] **Sidecar adapter** — JVM bridge (JDBC), Python analysis environment
- [x] **Plugin manager** — install, load, unload, dependency resolution
- [x] **Host functions** — plugin ↔ host API bridge
- [x] **Permission system** — capability-based plugin permissions

#### Caching & Performance

- [x] **Query cache** — result caching with TTL
- [x] **Metadata cache** — schema/table/column metadata caching
- [x] **LRU cache** — bounded memory cache with eviction
- [x] **Memory guard** — configurable memory limits
- [x] **Performance monitor** — runtime metrics collection
- [x] **Cache warming** — preemptive cache population

#### Logging & Diagnostics

- [x] **Structured logging** — configurable log levels, rotating files
- [x] **Log redaction** — automatic password/credential masking
- [x] **Log store** — persisted log querying

#### Cross-window & Layout

- [x] **dockview-vue** — VSCode-style IDE layout (panels, tabs, split views)
- [x] **Cross-window support** — popout editor panels to separate windows, merge back
- [x] **State sync** — cross-window editor state synchronization with conflict resolution
- [x] **Activity bar** — extension navigation sidebar
- [x] **Customizable layout** — drag-and-drop panel reordering

#### Security

- [x] **AES-256-GCM** — all passwords/credentials encrypted at rest
- [x] **Password masking** — URL credential redaction in logs
- [x] **Parameterized queries** — `query_with_params()` to prevent SQL injection
- [x] **Local-first** — zero telemetry, zero cloud sync, zero login

#### Database Migrations

- [x] **45 migrations** across 4 databases (global, project_meta, connection_metadata, project_analysis)
- [x] **Migration manager** — versioned schema evolution

### 🔧 In Progress / Planned

- [ ] **Schema-aware autocompletion** — table/column suggestions from metadata cache (spec drafted)
- [ ] **SQL explain plan** — database-native EXPLAIN visualization (spec drafted)
- [ ] **SQL dialect transpilation** — MySQL ↔ PostgreSQL ↔ SQLite via sqlglot (spec drafted)
- [ ] **Pagination** — server-side LIMIT/OFFSET for large result sets (spec drafted)
- [ ] **Frontend component tests** — vitest + @vue/test-utils test suite (spec drafted)
- [ ] **E2E integration tests** — full Tauri command test suite (spec drafted)
- [ ] **Multi-mode editor** — query/analysis/code mode separation (spec drafted)
- [ ] **Navigation bar optimization** — virtual tree performance, accessibility (spec drafted)
- [ ] **Properties panel** — database object detail panel (design drafted)

### 📊 Project Stats

| Category            | Count                                   |
| ------------------- | --------------------------------------- |
| Rust source files   | ~500+                                   |
| Tauri commands      | ~30                                     |
| Frontend extensions | 8 builtin                               |
| Vue components      | ~50+                                    |
| Database migrations | 45                                      |
| Rust test files     | 32                                      |
| Spec documents      | 13                                      |
| Supported databases | 4 (MySQL, PostgreSQL, SQLite, DuckDB)   |
| Auth methods        | 7                                       |
| Plugin runtimes     | 3 (Extism WASM, Sidecar, Tauri adapter) |

### ⚡ Quickstart

```bash
# Prerequisites: Rust, Node.js, Tauri CLI
git clone https://github.com/ling-love-xin/RdataStation.git
cd RdataStation
npm install
cargo tauri dev
```

If you need a stable, production-ready database tool, DBeaver, DataGrip, Navicat, and TablePlus are all excellent choices with years of refinement behind them.

If RdataStation's direction resonates with you — making analysis happen after the query, isolating projects while sharing common assets, keeping data entirely local — then you're welcome to follow along, try it out, share feedback, or even contribute.

🔍 How we differ
RdataStation is not trying to be a "better DBeaver" or a "better DataGrip." Those tools are already excellent at what they do. RdataStation simply focuses on a different part of the workflow. That doesn't make one approach right and the other wrong — just different starting points for different needs.

Traditional DB tools RdataStation Why it matters
Core focus Connecting to databases, writing SQL, displaying results What happens after the results come back You don't need another tool just to explore your query results
Analysis engine Not built-in. Users export data to Excel, Python, or BI tools DuckDB embedded in the core Analysis stays inside the tool. No export, no waiting
Performance Remote query execution only. Repeated analysis hits the remote database every time DuckDB local acceleration. First query fetches data; all subsequent analysis runs locally in milliseconds The first query is the only round trip; every follow-up is local
Result-set workflow Display results. Export if you need more Three modes: instant filtering, SQL re-query, DuckDB deep analysis — all in one panel From "just browsing" to "deep analysis" without switching tools
Data architecture Flat connection list. No project isolation Two-tier: project-level isolation + app-level shared services Your marketing analysis doesn't accidentally interfere with your inventory analysis
Shared assets No built-in mechanism. Manual file exchange Dimension tables and master data can be promoted to the app level, versioned, and reused Clean once, use everywhere. No more three copies of the same supplier table
Data sovereignty Most tools now push cloud sync, telemetry, or login Local-first. No cloud. No telemetry Your database passwords and analysis results never leave your machine
Extensibility Either closed ecosystem or traditional plugin model Extism WASM sandbox for lightweight plugins + Sidecar for heavy dependencies. Multi-language support Safe community extensions + isolated heavy runtimes, both in one architecture
🤝 Getting involved
All contributions are welcome, no matter your background.

💬 Just want to share your thoughts? Open a Discussion — feedback on the idea itself is valuable at this stage

🐛 Found a bug or have a feature idea? Open an Issue

🔧 Want to contribute code? Check our good first issues or read the design documents to understand the architecture

🌏 Not a native English speaker? Help us improve this README's translation — every correction counts

⭐ Like the idea? A star helps others discover the project

📄 License
Dual-licensed under MIT / Apache-2.0.
