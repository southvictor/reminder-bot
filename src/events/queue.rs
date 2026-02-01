use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Event {
    NotifyRequested {
        text: String,
        user_id: String,
        channel_id: String,
    },
    PendingConfirmed {
        pending_id: String,
        user_id: String,
    },
    PendingCanceled {
        pending_id: String,
        user_id: String,
    },
    ContextSubmitted {
        pending_id: String,
        user_id: String,
        context: String,
    },
}

#[derive(Clone)]
pub struct EventBus {
    tx: mpsc::Sender<Event>,
}

impl EventBus {
    pub fn new(buffer: usize) -> (Self, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(buffer);
        (Self { tx }, rx)
    }

    pub async fn emit(&self, event: Event) {
        let _ = self.tx.send(event).await;
    }
}
