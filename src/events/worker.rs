use tokio::sync::mpsc;

use crate::handlers::action::{ActionEngine, ActionEvent};

pub async fn run_event_worker(mut rx: mpsc::Receiver<ActionEvent>, engine: ActionEngine) {
    while let Some(event) = rx.recv().await {
        engine.handle_event(event).await;
    }
}
