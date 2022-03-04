use frankenstein::Api;
use frankenstein::GetUpdatesParamsBuilder;
use frankenstein::SendMessageParamsBuilder;
use frankenstein::TelegramApi;
use reqwest::Url;
use serde::Deserialize;
use std::env;
use std::sync::mpsc;
use std::{thread, time};

const URL: &str = "https://www.hub-ez.com/Tracking/GetTracking?trackingNumber=";

#[allow(dead_code)]
#[derive(Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
struct ListTrackingDetails {
    desc: String,
    location_name: String,
    event_time: String,
}

impl Eq for ListTrackingDetails {}

impl PartialEq for ListTrackingDetails {
    fn eq(&self, other: &Self) -> bool {
        self.event_time == other.event_time
            && self.desc == other.desc
            && self.location_name == other.location_name
    }
}

struct BotUpdate {
    tracking_number: String,
    updates: Vec<ListTrackingDetails>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListHawbDetails {
    id: u32,
    hawb_number: String,
    hawb_status: u32,
    sender_country: String,
    receiver_country: String,
    list_tracking_details: Vec<ListTrackingDetails>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TrackingResponse {
    all_count: u32,
    no_record_count: u32,
    delivered_count: u32,
    in_transit_count: u32,
    unpickup_count: u32,
    list_hawb_details: Vec<ListHawbDetails>,
}

fn get_latest(url: &Url) -> Result<TrackingResponse, reqwest::Error> {
    let req = reqwest::blocking::Client::new().post(url.as_ref()).body("");
    match req.send() {
        Ok(e) => {
            let text = e.text().unwrap();
            let response: TrackingResponse = serde_json::from_str(&text).unwrap();
            Ok(response)
        }
        Err(e) => Err(e),
    }
}

fn parse_tracking_number() -> String {
    let args: Vec<String> = env::args().collect();
    match args.len() {
        2 => args[1].clone(),
        _ => {
            // TODO: replace with Result<>
            panic!("Usage: {} <tracking number>", args[0]);
        }
    }
}

fn parse_tracking_response(
    resp: &TrackingResponse,
    last: &Option<TrackingResponse>,
    bot_tx: &mpsc::Sender<BotUpdate>,
) {
    let details = match resp.list_hawb_details.len() {
        1 => &(resp.list_hawb_details[0]),
        _ => {
            panic!("Expected exactly one tracking detail information block");
        }
    };
    let updates: Vec<&ListTrackingDetails> = match last {
        Some(e) => {
            let last_details: &Vec<ListTrackingDetails> = match e.list_hawb_details.len() {
                1 => &e.list_hawb_details[0].list_tracking_details,
                _ => {
                    panic!("Expected exactly one tracking detail information block");
                }
            };
            let mut delta: Vec<&ListTrackingDetails> =
                details.list_tracking_details.iter().collect();
            delta.retain(|&x| !last_details.contains(x));
            delta
        }
        None => details.list_tracking_details.iter().collect(),
    };
    match updates.len() {
        0 => {
            println!("No updates...");
        }
        _ => {
            console_update(&details.hawb_number, &updates);
            send_bot_update(&details.hawb_number, &updates, bot_tx);
            println!();
        }
    };
}

fn format_updates(tracking_number: &str, details: &Vec<&ListTrackingDetails>) -> String {
    let mut update = format!("Updates for {}:\n", tracking_number);
    for (i, event) in details.iter().enumerate() {
        update.push_str(&format!(
            "  {}: {} at {}\n",
            i + 1,
            event.location_name,
            event.event_time
        ));
        update.push_str(&format!("      {}\n", event.desc));
    }
    update
}

fn console_update(tracking_number: &str, details: &Vec<&ListTrackingDetails>) {
    println!("{}", format_updates(tracking_number, details));
}

fn send_bot_update(
    tracking_number: &str,
    details: &Vec<&ListTrackingDetails>,
    bot_tx: &mpsc::Sender<BotUpdate>,
) {
    let update = BotUpdate {
        tracking_number: tracking_number.to_owned(),
        updates: details.iter().map(|x| x.to_owned().to_owned()).collect(),
    };
    match bot_tx.send(update) {
        Ok(_) => {}
        Err(e) => panic!("Error sending to bot thread: {}", e),
    };
}

fn start_bot(rx: mpsc::Receiver<BotUpdate>) {
    println!("Starting Telegram bot...");
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let target = env::var("TELEGRAM_BOT_USER").expect("TELEGRAM_BOT_USER not set");
    let api = Api::new(&token);
    loop {
        match rx.recv() {
            Ok(e) => {
                let msg = SendMessageParamsBuilder::default()
                    .chat_id(target.to_owned())
                    .text(format_updates(
                        &e.tracking_number,
                        &e.updates.iter().collect(),
                    ))
                    .build()
                    .unwrap();
                match api.send_message(&msg) {
                    Ok(e) => {
                        println!("Update sent");
                    }
                    Err(e) => {
                        println!("Error receiving update data: {:?}", e);
                    }
                }
            }
            Err(e) => {
                println!("Error receiving update data: {}", e);
            }
        }
    }
}

fn main() {
    let (bot_tx, bot_rx) = mpsc::channel::<BotUpdate>();
    let tracking_number = parse_tracking_number();
    println!("Checking tracking number: {}", tracking_number);
    let url = Url::parse(&(URL.to_owned() + &tracking_number));
    let sleep_time = time::Duration::from_millis(1000);
    let mut last_response: Option<TrackingResponse> = None;
    thread::spawn(move || start_bot(bot_rx));
    match url {
        Ok(url) => loop {
            match get_latest(&url) {
                Ok(e) => {
                    parse_tracking_response(&e, &last_response, &bot_tx);
                    last_response = Some(e);
                }
                Err(e) => {
                    panic!("Error encountered: {}", e);
                }
            }
            thread::sleep(sleep_time);
        },
        Err(e) => {
            panic!("Error parsing URL: {}", e);
        }
    };
}
