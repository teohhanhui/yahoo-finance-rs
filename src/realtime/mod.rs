use base64::decode;
use chrono::{DateTime, NaiveDateTime, Utc};
use futures_util::{StreamExt, SinkExt};
use protobuf::{ parse_from_bytes };
use serde::{ Serialize };
use std::{ collections::HashMap, sync::RwLock };
use tokio::net::TcpStream;
use tokio_tungstenite::{ connect_async, MaybeTlsStream, tungstenite::protocol::Message, tungstenite::Result, WebSocketStream };

mod data;
use data::PricingData;

mod quote;
pub use quote::{ Quote, QuoteType, TradingSession };

#[derive(Debug, Clone, Serialize)]
struct Subs<'a> { subscribe: Vec<&'a str> }

/// Realtime price quote streamer
/// 
/// To use it:
/// 1. Create a new streamer with `Streamer::new().await;`
/// 1. Subscribe to some symbols with `streamer.subscribe(vec!["AAPL"], |quote| /* do something */).await;`
/// 1. Let the streamer run `streamer.run().await;`
pub struct Streamer<'a> {
   stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
   subscriptions: RwLock<HashMap<&'a str, Box<dyn Fn(Quote) + 'static>>>
}
impl<'a> Streamer<'a> {
   /// Create a new realtime price quote streamer and make the initial connection to Yahoo for data
   pub async fn new() -> Streamer<'a> {
      let (stream, _) = connect_async("wss://streamer.finance.yahoo.com").await.expect("Failed to connect");
      Streamer {
         stream: stream,
         subscriptions: RwLock::new(HashMap::new())
      }
   }

   /// Create a new realtime price quote streamer and make the initial connection to Yahoo for data
   pub async fn run(&mut self) -> Result<()> {
      // build up the subscription list
      let mut v = Vec::new();
      {
         let map = self.subscriptions.read().unwrap();
         for (symbol, _) in map.iter() { v.push(*symbol); }
      }
 
      // and subscribe to symbols
      self.stream.send(Message::Text(serde_json::to_string(&Subs { subscribe: v }).unwrap())).await?;

      // our main run loop - look at messages, and if it's for something good, invoke
      // the callback with quote information
      while let Some(msg) = self.stream.next().await {
         let msg = msg?;
         let x = parse_from_bytes::<PricingData>(&decode(msg.into_data()).unwrap()).unwrap();
         
         let map = self.subscriptions.read().expect("Can't read subscriptions");
         match map.get(x.id.as_str()) {
            Some(callback) => callback(Quote {
               symbol: x.id.clone(),
               quote_type: QuoteType::from_pd(x.quoteType),
               timestamp: DateTime::from_utc(NaiveDateTime::from_timestamp(x.time, 0), Utc),
               session: TradingSession::from_pd(x.marketHours),
               price: x.price,
               volume: x.dayVolume
            }),
            None => ()
         }
      }
   
      Ok(())
   }

   /// Subscribe to changes on one or more symbols
   pub async fn subscribe(&mut self, symbols: Vec<&'a str>, callback: impl Fn(Quote) + 'static + Copy) {
      let mut map = self.subscriptions.write().expect("Can't lock subscriptions");

      for symbol in symbols {
         if !map.contains_key(symbol) { map.insert(symbol, Box::new(callback)); }
      }

      // later - subscribe to symbols if we are in a 'running' state
   }
}
