#![allow(dead_code)]

use chrono;

pub async fn run_calendar_loop() {
    // TODO: Hook into real calendar client.
}


struct CalendarEvent {
    pub title: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub description: Option<String>,
}

trait CalendarClient {
    async fn get_events_for_day(&self, day: chrono::NaiveDate) -> Vec<CalendarEvent>;
    async fn create_event(&self, event: CalendarEvent) -> Result<(), Box<dyn std::error::Error>>;
}
