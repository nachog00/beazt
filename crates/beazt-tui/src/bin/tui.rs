//! Composition root for the TUI surface: terminal lifecycle, event loop, and
//! the concrete [`ZainoClient`] adapter — none of which the tested library
//! half (`beazt_tui::{AppState, render}`) knows about.
//!
//! Config: `ZAINO_RPC_URL` (default `http://127.0.0.1:8232`) and
//! `BEAZT_USD_PRICE` (unset by default; opts into a live CoinGecko-backed
//! ZEC/USD price), same as the web surface. `RUST_LOG` (default `info`)
//! controls log verbosity; logs go to `BEAZT_LOG_DIR`/`beazt-tui.log`
//! (`BEAZT_LOG_DIR` defaults to `.`), never to stdout/stderr — those are
//! the terminal's own canvas while the alternate screen is active. `q`
//! quits from anywhere. On the home screen: `t` starts a
//! txid lookup, `a` starts an address lookup, `b` starts a block
//! height/hash lookup, `m` looks up the mempool directly, `n` looks up
//! node diagnostics directly, and the chain height refreshes every 5s.
//! While typing: characters append, Backspace edits, Enter looks it up,
//! Esc cancels back home. On the block screen: `s` looks up its
//! treestate. On the treestate screen: `r` looks up the Sapling pool's
//! subtree roots. On the transaction/address/mempool/node-info/
//! subtree-roots screen: Esc goes back home.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use beazt_domain::NullPriceOracle;
use beazt_price_client::CoinGeckoPriceOracle;
use beazt_tui::{render, AppState, Screen};
use beazt_zaino_client::ZainoClient;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use jsonrpsee::http_client::HttpClientBuilder;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// How often the CoinGecko-backed oracle refreshes its cached price, when
/// enabled. Cosmetic data — no need for anything tighter.
const PRICE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() {
    // Never stdout/stderr: that's the alternate screen's own canvas while
    // the TUI is running, so logging there would corrupt the UI instead
    // of helping diagnose it.
    let log_dir = std::env::var("BEAZT_LOG_DIR").unwrap_or_else(|_| ".".to_string());
    let file_appender = tracing_appender::rolling::never(&log_dir, "beazt-tui.log");
    let (non_blocking, _log_guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let rpc_url =
        std::env::var("ZAINO_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8232".to_string());
    let client = HttpClientBuilder::default()
        .build(&rpc_url)
        .unwrap_or_else(|e| panic!("invalid ZAINO_RPC_URL {rpc_url:?}: {e}"));
    let reader = ZainoClient::new(client);

    let mut state = AppState::default();
    if std::env::var("BEAZT_USD_PRICE").is_ok() {
        state.set_price_oracle(Arc::new(CoinGeckoPriceOracle::spawn(
            PRICE_REFRESH_INTERVAL,
        )));
    } else {
        state.set_price_oracle(Arc::new(NullPriceOracle));
    }

    let mut terminal = setup_terminal().expect("set up terminal");
    let result = run(&mut terminal, &reader, state).await;
    teardown_terminal(&mut terminal).expect("restore terminal");
    result.expect("run loop");
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

fn teardown_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    Ok(())
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    reader: &ZainoClient,
    mut state: AppState,
) -> io::Result<()> {
    state.refresh(reader).await;
    let mut last_refresh = tokio::time::Instant::now();

    loop {
        terminal.draw(|frame| render(frame, &state))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    return Ok(());
                }
                match state.screen() {
                    Screen::Home => match key.code {
                        KeyCode::Char('t') => state.start_txid_input(),
                        KeyCode::Char('a') => state.start_address_input(),
                        KeyCode::Char('b') => state.start_block_input(),
                        KeyCode::Char('m') => state.lookup_mempool(reader).await,
                        KeyCode::Char('n') => state.lookup_node_diagnostics(reader).await,
                        _ => {}
                    },
                    Screen::EnterTxid(_) => match key.code {
                        KeyCode::Char(c) => state.push_char(c),
                        KeyCode::Backspace => state.backspace(),
                        KeyCode::Enter => state.lookup_transaction(reader).await,
                        KeyCode::Esc => state.go_home(),
                        _ => {}
                    },
                    Screen::EnterAddress(_) => match key.code {
                        KeyCode::Char(c) => state.push_char(c),
                        KeyCode::Backspace => state.backspace(),
                        KeyCode::Enter => state.lookup_address(reader).await,
                        KeyCode::Esc => state.go_home(),
                        _ => {}
                    },
                    Screen::EnterBlock(_) => match key.code {
                        KeyCode::Char(c) => state.push_char(c),
                        KeyCode::Backspace => state.backspace(),
                        KeyCode::Enter => state.lookup_block(reader).await,
                        KeyCode::Esc => state.go_home(),
                        _ => {}
                    },
                    Screen::Block(_, _) => match key.code {
                        KeyCode::Char('s') => state.lookup_treestate(reader).await,
                        KeyCode::Esc => state.go_home(),
                        _ => {}
                    },
                    Screen::Treestate(_, _) => match key.code {
                        KeyCode::Char('r') => state.lookup_subtree_roots(reader).await,
                        KeyCode::Esc => state.go_home(),
                        _ => {}
                    },
                    Screen::Transaction(_, _)
                    | Screen::Address(_, _)
                    | Screen::Mempool(_)
                    | Screen::NodeInfo(_)
                    | Screen::SubtreeRoots(_, _) => {
                        if key.code == KeyCode::Esc {
                            state.go_home();
                        }
                    }
                }
            }
        }

        if matches!(state.screen(), Screen::Home) && last_refresh.elapsed() >= REFRESH_INTERVAL {
            state.refresh(reader).await;
            last_refresh = tokio::time::Instant::now();
        }
    }
}
