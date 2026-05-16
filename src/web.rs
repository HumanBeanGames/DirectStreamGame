use crate::{
    constants::{STREAM_PATH, WEB_ADDR},
    frames::EncodedFrameHub,
    stats::SharedStats,
};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};

pub(crate) fn start_local_web_server(frame_hub: EncodedFrameHub, stats: SharedStats) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(WEB_ADDR) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("Could not bind local stream page at http://{WEB_ADDR}: {err}");
                return;
            }
        };

        eprintln!("Local stream page: http://{WEB_ADDR}");

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let frame_hub = frame_hub.clone();
                    let stats = stats.clone();
                    thread::spawn(move || handle_web_request(stream, frame_hub, stats));
                }
                Err(err) => eprintln!("Local web server connection failed: {err}"),
            }
        }
    });
}

fn handle_web_request(mut stream: TcpStream, frame_hub: EncodedFrameHub, stats: SharedStats) {
    let mut request = [0; 1024];
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let bytes_read = stream.read(&mut request).unwrap_or(0);
    let request = String::from_utf8_lossy(&request[..bytes_read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    if path.starts_with(STREAM_PATH) {
        stream_mjpeg(stream, frame_hub, stats);
    } else {
        stats.with_mut(|stats| stats.preview_requests += 1);
        serve_preview_page(stream);
    }
}

fn serve_preview_page(mut stream: TcpStream) {
    let body = stream_page_html();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let _ = stream.write_all(response.as_bytes());
}

fn stream_mjpeg(mut stream: TcpStream, frame_hub: EncodedFrameHub, stats: SharedStats) {
    stats.with_mut(|stats| stats.stream_clients += 1);
    let header = "HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=frame\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
    if stream.write_all(header.as_bytes()).is_err() {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    }

    let mut last_sequence = 0;
    while let Some((sequence, jpeg)) = frame_hub.wait_for_frame_after(last_sequence) {
        last_sequence = sequence;
        let part_header = format!(
            "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
            jpeg.len()
        );

        if stream.write_all(part_header.as_bytes()).is_err()
            || stream.write_all(&jpeg).is_err()
            || stream.write_all(b"\r\n").is_err()
        {
            break;
        }
    }
    stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
}

fn stream_page_html() -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Direct Stream Game</title>
  <style>
    :root {{ color-scheme: dark; font-family: Arial, sans-serif; background: #111318; color: #eef3f8; }}
    body {{ margin: 0; min-height: 100vh; display: grid; grid-template-rows: auto 1fr; }}
    header {{ padding: 12px 16px; background: #1b2029; border-bottom: 1px solid #303847; }}
    main {{ display: grid; gap: 12px; place-items: center; padding: 16px; }}
    img {{ width: min(100%, 960px); aspect-ratio: 4 / 3; object-fit: contain; image-rendering: pixelated; background: #050608; border: 1px solid #303847; }}
  </style>
</head>
<body>
  <header>Direct Stream Game local preview</header>
  <main>
    <img id="stream" alt="Bevy GPU readback stream" src="{STREAM_PATH}">
  </main>
  <script>
    const stream = document.getElementById("stream");
    stream.onerror = () => setTimeout(() => {{
      stream.src = "{STREAM_PATH}?retry=" + Date.now();
    }}, 1000);
  </script>
</body>
</html>"#
    )
}
