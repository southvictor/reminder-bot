use reminderBot::service::routing::{route_intent, Intent};

#[test]
fn routes_notification_when_time_tokens_present() {
    let result = route_intent("remind me tomorrow at 3 to call mom");
    assert_eq!(result.intent, Intent::Notification);
}

#[test]
fn routes_unknown_when_no_time_tokens_present() {
    let result = route_intent("buy milk and eggs");
    assert_eq!(result.intent, Intent::Unknown);
}

#[test]
fn routes_notification_for_month_dates() {
    let result = route_intent("pay rent on March 5");
    assert_eq!(result.intent, Intent::Notification);
}

#[test]
fn routes_notification_for_am_pm_times() {
    let result = route_intent("call mom 5pm");
    assert_eq!(result.intent, Intent::Notification);
}
