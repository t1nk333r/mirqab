//! A page with nothing to show yet: a title and a line under it, in an
//! `adw::StatusPage`.

pub fn empty_state(title: &str, body: &str) -> adw::StatusPage {
    adw::StatusPage::builder()
        .title(title)
        .description(body)
        .vexpand(true)
        .build()
}
