use tokio::sync::mpsc;

use crate::action::ActionEvent;

#[derive(Clone)]
pub struct EventBus {
    tx: mpsc::Sender<ActionEvent>,
}

impl EventBus {
    pub fn new(buffer: usize) -> (Self, mpsc::Receiver<ActionEvent>) {
        let (tx, rx) = mpsc::channel(buffer);
        (Self { tx }, rx)
    }

    pub async fn emit(&self, event: ActionEvent) {
        let _ = self.tx.send(event).await;
    }
}
