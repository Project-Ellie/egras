use egras::audit::model::AuditEvent;
use egras::audit::service::{AuditRecorder, ChannelAuditRecorder, RecorderError};
use tokio::sync::mpsc;
use uuid::Uuid;

#[tokio::test]
async fn records_into_channel() {
    let (tx, mut rx) = mpsc::channel(4);
    let rec = ChannelAuditRecorder::new(tx);

    let e = AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7());
    rec.record(e.clone()).await.unwrap();

    let got = rx.recv().await.unwrap();
    assert_eq!(got.id, e.id);
    assert_eq!(got.event_type, "login.success");
}

#[tokio::test]
async fn returns_channel_full_when_buffer_exhausted() {
    let (tx, _rx) = mpsc::channel(1);
    let rec = ChannelAuditRecorder::new(tx);

    // First send fills the buffer.
    rec.record(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap();
    // Second send returns ChannelFull.
    let err = rec.record(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7())).await.unwrap_err();
    assert!(matches!(err, RecorderError::ChannelFull));
}

#[tokio::test]
async fn returns_closed_after_receiver_drop() {
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let rec = ChannelAuditRecorder::new(tx);
    let err = rec.record(AuditEvent::login_success(Uuid::now_v7(), Uuid::now_v7()))
        .await.unwrap_err();
    assert!(matches!(err, RecorderError::Closed));
}
