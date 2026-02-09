use reminderBot::service::routing::{HeuristicRouter, Intent, IntentRouter};

#[tokio::test]
async fn routes_notification_when_time_tokens_present() {
    let router = HeuristicRouter;
    let result = router.route("notify me tomorrow at 3 to call mom").await;
    assert_eq!(result.intent, Intent::Notification);
}

#[tokio::test]
async fn routes_unknown_when_no_time_tokens_present() {
    let router = HeuristicRouter;
    let result = router.route("buy milk and eggs").await;
    assert_eq!(result.intent, Intent::Unknown);
}

#[tokio::test]
async fn routes_notification_for_month_dates() {
    let router = HeuristicRouter;
    let result = router.route("pay rent on March 5").await;
    assert_eq!(result.intent, Intent::Notification);
}

#[tokio::test]
async fn routes_notification_for_am_pm_times() {
    let router = HeuristicRouter;
    let result = router.route("call mom 5pm").await;
    assert_eq!(result.intent, Intent::Notification);
}
