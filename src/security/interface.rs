use axum::Router;
use crate::app_state::AppState;

pub fn public_router() -> Router<AppState> {
    Router::new()
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
}
