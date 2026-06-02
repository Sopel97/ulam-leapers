use std::time::Duration;

pub fn format_opt_duration_hhmmss(duration: Option<Duration>) -> String {
    if let Some(duration) = duration {
        format_duration_hhmmss(duration)
    } else {
        "N/A".to_string()
    }
}

pub fn format_duration_hhmmss(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let seconds = total_secs % 60;
    let minutes = (total_secs / 60) % 60;
    let hours = (total_secs / 60) / 60;
    format!("{:0>2}:{:0>2}:{:0>2}", hours, minutes, seconds)
}
