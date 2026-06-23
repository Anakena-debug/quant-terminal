# Quant Terminal (`qterm`)

A Bloomberg-style terminal for quantitative analysts — a fast, keyboard-driven TUI built in
Rust with [ratatui](https://ratatui.rs). Live market data from **Interactive Brokers**, with a
built-in **simulated feed** so it runs beautifully out of the box with zero setup.

```
┌ QUANT TERMINAL ──────────────── 14:32 ET ── ● LIVE ┐
│ > AAPL GP▮                                          │
├────────────┬───────────────────────────────────────┤
│ WATCHLIST  │ AAPL  228.51 ▲ +1.24 (+0.55%)         │
│ AAPL 228.51│  235┤                          ╭╮      │
│ MSFT 421.02│  228┤            ╭─╮    ╭───╮ ╭╯╰─╮    │  ← candles + SMA/EMA/Bollinger
│ NVDA 131.24│  221┤    ╭───╮╭─╯ ╰────╯   ╰─╯   ╰    │
│ TSLA 248.91│      └──────────────────────────────── │
├────────────┤  RSI 58  MACD ▲  vol ▁▂▃▅▇▆  SMA20 …  │  ← indicator sub-panes
│ NEWS …     │  Bid 228.49  Ask 228.53  H/L  Vol 52M  │  ← stats
└────────────┴───────────────────────────────────────┘
 Tab panel  ↑↓ sel  1-5 tf  c chart  / cmd  ? help  q quit
```

## Quick start

```bash
cargo run
```

That's it — it launches with the **simulated** data feed: live-ticking watchlist, candlestick
charts, and technical indicators, no accounts or API keys required. Run it in a real terminal
(it needs a TTY).

## Features

- **Live market monitor** — color-coded watchlist with last/change%/volume and per-symbol
  sparklines, updating in real time.
- **Charting** — candlestick or line charts (`c` to toggle) across 5 timeframes, with
  **SMA(20)**, **EMA(50)**, and **Bollinger Bands** overlays drawn on a Braille canvas.
- **Technical analysis** — **RSI(14)** and **MACD** sub-panes, a volume histogram, and live
  numeric readouts.
- **Screens** — switch between **Monitor**, **Portfolio**, **Backtest**, **Risk**, and
  **Optimizer** with `F1`–`F5` (or the `mon`/`port`/`bt`/`risk`/`opt` commands).
- **Portfolio** — account summary (NetLiq, cash, buying power) + a positions table with live
  market value and unrealized P&L (from IBKR, or a demo book in simulated mode).
- **Backtest / strategy lab** — long-only backtester over the active symbol with five strategies
  (SMA crossover, mean-reversion z-score, momentum, Bollinger, RSI — `s` to cycle): equity curve
  vs buy-&-hold, return/MaxDD/Sharpe/win-rate, plus an **SMA parameter-sweep heatmap**.
- **Risk dashboard** — a **correlation heatmap** across the watchlist + per-symbol stats:
  annualized vol, beta vs SPY, Sharpe/Sortino, historical & parametric **VaR**, max drawdown,
  skew, and kurtosis.
- **Portfolio optimizer** — **Markowitz mean-variance**: a long-only efficient-frontier cloud
  with the assets, the min-variance and max-Sharpe portfolios, and the optimal weights.
- **Options** — **Black-Scholes** pricing, the Greeks (Δ Γ ν Θ), and an **implied-vol solver**
  (Newton + bisection): an IV smile/skew chart and a greeks chain. The math is exact; the chain
  is synthetic for now (a live IBKR option chain is next).
- **Command line** — Bloomberg-style: type `AAPL GP` to graph a symbol, `add NVDA`, `tf 6M`, etc.
- **Interactive Brokers** — real quotes and history via IB Gateway / TWS, with automatic
  fallback to the simulated feed if the gateway isn't reachable.
- **ET market clock**, connection/data-quality badge (`LIVE` / `DELAYED` / `SIM`), and a polished
  dark + amber theme.

## Keys

| Key | Action |
|---|---|
| `Tab` / `Shift-Tab` | cycle panel focus |
| `↑ ↓` or `k j` | move watchlist selection |
| `1 2 3 4 5` | timeframe — 1D / 5D / 1M / 6M / 1Y |
| `F1` – `F6` | screen — Monitor / Portfolio / Backtest / Risk / Optimizer / Options |
| `c` | toggle candle / line chart |
| `s` | cycle backtest strategy |
| `/` or `:` | focus the command line |
| `?` | help overlay |
| `q` / `Ctrl-C` | quit |

## Commands (type after `/`)

| Command | Effect |
|---|---|
| `AAPL` or `AAPL GP` | load a symbol's chart (adds it if new) |
| `add NVDA` | add a symbol to the watchlist |
| `rm TSLA` | remove a symbol |
| `tf 6M` | set timeframe (`1D 5D 1M 6M 1Y`) |
| `chart line` / `chart candle` | set chart style |
| `port` `bt` `risk` `opt` `options` `mon` | switch screen |
| `q` | quit |

Any ticker works in simulated mode — prices are derived deterministically from the symbol.

## Interactive Brokers (live data)

1. Install and launch **IB Gateway** (lighter) or **Trader Workstation**.
2. Enable the API: *Configuration → API → Settings* → check *Enable ActiveX and Socket Clients*,
   and add `127.0.0.1` to *Trusted IPs*.
3. Note the port: IB Gateway paper `4002` / live `4001`; TWS paper `7497` / live `7496`.
4. Point the app at it via `config.toml` (or a gitignored `config.local.toml`):

```toml
provider = "ibkr"

[ibkr]
host = "127.0.0.1"
port = 4002
client_id = 100
delayed = true   # paper accounts usually get delayed data
```

Live quotes use 5-second real-time bars; history uses `historical_data`. Live data depends on
your IBKR market-data subscriptions. If the gateway is unreachable the app logs the error and
**falls back to the simulated feed** so it always runs.

> v1 is **read-only** — it places no orders.

## Configuration

`config.toml` is the committed sample; copy it to `config.local.toml` (gitignored) for personal
settings. Both support `provider`, `watchlist`, and the `[ibkr]` block. Missing values fall back
to sensible defaults.

## Logging

The TUI owns stdout, so logs go to `logs/quantterm.log`. Set the level with `QUANTTERM_LOG`
(or `RUST_LOG`), e.g. `QUANTTERM_LOG=debug cargo run`.

## Preview without a terminal

Render the real UI to stdout (no TTY needed) — handy for screenshots, CI, or remote shells:

```bash
cargo run -- --snapshot 120x36            # monospace text
cargo run -- --snapshot 120x36 line help  # line chart + help overlay
cargo run -- --snapshot 120x36 svg > ui.svg   # colorized SVG (also: ansi)
```

## Architecture

Async (tokio) event loop → `Action` reducer → ratatui render. Market data sits behind a single
`DataProvider` trait so the source is swappable.

```
src/
├── main.rs            entry: logging, config, terminal, run loop
├── event.rs           async event loop (input thread + tick/render → mpsc)
├── action.rs          user-intent actions
├── app.rs             App state, input mapping, command execution
├── update.rs          the reducer (Action → state)
├── config.rs          config.toml loading
├── command/           Bloomberg-style command parser
├── data/              DataProvider trait
│   ├── simulated.rs   deterministic random-walk feed (default)
│   ├── ibkr.rs        Interactive Brokers via the `ibapi` crate
│   └── manager.rs     streams provider events into the loop
├── analytics/         indicators (SMA/EMA/Bollinger/RSI/MACD) + ChartSeries
│   ├── stats.rs       returns, vol, corr/cov, beta, Sharpe/Sortino, VaR, skew/kurt
│   ├── quant.rs       correlation matrix, risk stats, Markowitz optimizer
│   └── options.rs     Black-Scholes greeks + implied-vol solver + chain
├── backtest.rs        strategies (SMA/mean-rev/momentum/Bollinger/RSI) + SMA sweep
└── ui/                rendering
    ├── theme.rs       palette + ET market clock
    ├── chart.rs       candlestick/line canvas + indicator panes
    ├── portfolio.rs   account summary + positions table
    ├── backtest.rs    strategy stats + equity curve + sweep heatmap
    ├── risk.rs        correlation heatmap + risk-stats table
    ├── optimizer.rs   efficient-frontier scatter + optimal weights
    └── options.rs     IV smile + Black-Scholes greeks chain
```

## Development

```bash
cargo run                       # simulated feed
cargo test                      # unit + render-smoke tests
cargo clippy --all-targets      # lints (clean)
cargo fmt
```

Tests cover the indicators, the command parser, the config loader, the simulated feed, the
backtest engine, and a `TestBackend` render-smoke test that draws the full UI at several sizes.

## Roadmap

Shipped: live monitor, charting + TA, command line, IBKR + simulated providers, multi-screen
navigation, account/positions view, strategy backtester + parameter sweep, a correlation /
risk dashboard (vol, beta, Sharpe/Sortino, VaR, skew/kurtosis), a Markowitz optimizer, and
Black-Scholes options analytics (greeks + implied-vol + smile).
Deferred (the architecture leaves room): a live IBKR option chain, Monte-Carlo simulation,
pairs/cointegration, factor/regression analysis, security deep-dive & screener, walk-forward
optimization, L2 depth ladder, news, and order entry.

## License

MIT — see [`LICENSE`](LICENSE). Contributions welcome.

## Disclaimer

This project is **not affiliated with, endorsed by, or sponsored by** Bloomberg L.P. or
Interactive Brokers. "Bloomberg" and "IBKR" are trademarks of their respective owners, used here
only descriptively. The software is provided **as is, for research and educational purposes**, and
is **not financial advice**. It is read-only (it places no orders), but you are solely responsible
for how you use it and for any market-data subscriptions or API access. Use at your own risk.
