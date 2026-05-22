use crate::{
    audio::CustomAudioPacketHub,
    chat::LocalChatHub,
    constants::{
        AUDIO_STREAM_PATH, CUSTOM_AUDIO_CHANNELS, CUSTOM_AUDIO_SAMPLE_RATE, LOCAL_CHAT_FEED_PATH,
        LOCAL_CHAT_PATH, PALETTE_STREAM_PATH, STREAM_HEIGHT, STREAM_PATH, STREAM_STATUS_PATH,
        STREAM_WIDTH, WEB_ADDR,
    },
    frames::EncodedFrameHub,
    palette::PaletteFrameHub,
    stats::SharedStats,
    stream_control::CustomStreamState,
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    thread,
    time::Duration,
};

const CUSTOM_STREAM_SERVER_DELAY: Duration = Duration::ZERO;
const CUSTOM_STREAM_PLAYBACK_BUFFER_SECONDS: f64 = 1.0;

#[derive(Clone)]
pub(crate) enum LocalStreamSource {
    Mjpeg(EncodedFrameHub),
    Palette {
        frames: PaletteFrameHub,
        audio: CustomAudioPacketHub,
        chat: LocalChatHub,
        active: CustomStreamState,
    },
}

pub(crate) fn start_local_web_server(source: LocalStreamSource, stats: SharedStats) {
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
                    let source = source.clone();
                    let stats = stats.clone();
                    thread::spawn(move || handle_web_request(stream, source, stats));
                }
                Err(err) => eprintln!("Local web server connection failed: {err}"),
            }
        }
    });
}

fn handle_web_request(mut stream: TcpStream, source: LocalStreamSource, stats: SharedStats) {
    let peer_addr = stream.peer_addr().ok();
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let request = read_http_request(&mut stream);
    let request = String::from_utf8_lossy(&request);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let method = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("GET");

    if method.eq_ignore_ascii_case("OPTIONS") {
        serve_options(stream);
        return;
    }

    if path.starts_with(STREAM_PATH) {
        if let LocalStreamSource::Mjpeg(frame_hub) = source {
            stream_mjpeg(stream, frame_hub, stats);
        } else {
            serve_not_found(stream);
        }
    } else if path.starts_with(PALETTE_STREAM_PATH) {
        if let LocalStreamSource::Palette { frames, active, .. } = source {
            stream_palette(stream, frames, stats, active);
        } else {
            serve_not_found(stream);
        }
    } else if path.starts_with(AUDIO_STREAM_PATH) {
        if let LocalStreamSource::Palette { audio, active, .. } = source {
            stream_pcm_audio(stream, audio, stats, active);
        } else {
            serve_not_found(stream);
        }
    } else if path.starts_with(LOCAL_CHAT_FEED_PATH) {
        if let LocalStreamSource::Palette { chat, .. } = source {
            serve_local_chat_feed(stream, path, chat);
        } else {
            serve_not_found(stream);
        }
    } else if path.starts_with(LOCAL_CHAT_PATH) {
        if let LocalStreamSource::Palette { chat, .. } = source {
            submit_local_chat(stream, &request, peer_addr, chat);
        } else {
            serve_not_found(stream);
        }
    } else if path.starts_with(STREAM_STATUS_PATH) {
        if let LocalStreamSource::Palette { active, .. } = source {
            serve_stream_status(stream, active);
        } else {
            serve_not_found(stream);
        }
    } else {
        stats.with_mut(|stats| stats.preview_requests += 1);
        serve_preview_page(stream, &source);
    }
}

fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::with_capacity(4096);
    let mut buffer = [0; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let Ok(bytes_read) = stream.read(&mut buffer) else {
            break;
        };
        if bytes_read == 0 {
            break;
        }

        request.extend_from_slice(&buffer[..bytes_read]);
        if header_end.is_none() {
            header_end = find_header_end(&request);
            if let Some(end) = header_end {
                content_length = parse_content_length(&request[..end]);
            }
        }

        if let Some(end) = header_end {
            if request.len() >= end + content_length {
                break;
            }
        }

        if request.len() > 64 * 1024 {
            break;
        }
    }

    request
}

fn find_header_end(request: &[u8]) -> Option<usize> {
    request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn parse_content_length(headers: &[u8]) -> usize {
    let headers = String::from_utf8_lossy(headers);
    headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0)
}

fn serve_preview_page(mut stream: TcpStream, source: &LocalStreamSource) {
    let body = match source {
        LocalStreamSource::Mjpeg(_) => mjpeg_stream_page_html(),
        LocalStreamSource::Palette { .. } => palette_stream_page_html(),
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let _ = stream.write_all(response.as_bytes());
}

fn serve_not_found(mut stream: TcpStream) {
    let body = "Not found";
    let response = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_stream_status(mut stream: TcpStream, active: CustomStreamState) {
    let body = if active.is_active() {
        r#"{"online":true}"#
    } else {
        r#"{"online":false}"#
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store, no-cache, must-revalidate, max-age=0\r\nPragma: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn submit_local_chat(
    mut stream: TcpStream,
    request: &str,
    peer_addr: Option<SocketAddr>,
    chat: LocalChatHub,
) {
    let Some((_, body)) = request.split_once("\r\n\r\n") else {
        serve_bad_request(stream);
        return;
    };
    let message = body.trim();
    if message.is_empty() || message.len() > 500 {
        serve_bad_request(stream);
        return;
    }
    let identity = local_chat_identity(request, peer_addr);
    let Some(display_name) = chat.submit(identity, message.to_owned()) else {
        serve_forbidden(stream);
        return;
    };
    let body = format!(r#"{{"ok":true,"name":"{}"}}"#, json_escape(&display_name));
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store, no-cache, must-revalidate, max-age=0\r\nPragma: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_local_chat_feed(mut stream: TcpStream, path: &str, chat: LocalChatHub) {
    let after = query_param(path, "after")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let generation = chat.generation();
    let latest_id = chat.latest_id();
    let entries = chat.entries_after(after);
    let mut body = format!(r#"{{"generation":{generation},"latest":{latest_id},"messages":["#);
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&format!(
            r#"{{"id":{},"name":"{}","text":"{}"}}"#,
            entry.id,
            json_escape(&entry.display_name),
            json_escape(&entry.text)
        ));
    }
    body.push_str("]}");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn query_param(path: &str, name: &str) -> Option<String> {
    let (_, query) = path.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (key == name).then(|| value.to_owned())
    })
}

fn local_chat_identity(request: &str, peer_addr: Option<SocketAddr>) -> String {
    header_value(request, "cf-connecting-ip")
        .or_else(|| header_value(request, "x-forwarded-for").and_then(first_forwarded_ip))
        .or_else(|| peer_addr.map(|addr| addr.ip().to_string()))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn header_value(request: &str, name: &str) -> Option<String> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn first_forwarded_ip(value: String) -> Option<String> {
    value
        .split(',')
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn serve_bad_request(mut stream: TcpStream) {
    let body = "bad request";
    let response = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_forbidden(mut stream: TcpStream) {
    let body = "forbidden";
    let response = format!(
        "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_options(mut stream: TcpStream) {
    let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Max-Age: 86400\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
}

fn stream_mjpeg(mut stream: TcpStream, frame_hub: EncodedFrameHub, stats: SharedStats) {
    stats.with_mut(|stats| stats.stream_clients += 1);
    let header = "HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=frame\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n";
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

fn stream_palette(
    mut stream: TcpStream,
    frame_hub: PaletteFrameHub,
    stats: SharedStats,
    active: CustomStreamState,
) {
    if !active.is_active() {
        let body = "stream offline";
        let response = format!(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
        return;
    }

    stats.with_mut(|stats| stats.stream_clients += 1);
    let stream_fps = active.fps();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Expose-Headers: X-Stream-Fps, X-Playback-Buffer-Seconds\r\nConnection: close\r\nX-Stream-Fps: {stream_fps}\r\nX-Playback-Buffer-Seconds: {CUSTOM_STREAM_PLAYBACK_BUFFER_SECONDS:.3}\r\n\r\n"
    );
    if stream.write_all(header.as_bytes()).is_err() {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    }

    stats.with_mut(|stats| stats.custom_stage = "waiting for stream frame");
    let Some(start_batch) =
        frame_hub.wait_for_delayed_encoded_start_batch(CUSTOM_STREAM_SERVER_DELAY, &stats, || {
            active.is_active()
        })
    else {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    };
    if !active.is_active() {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    }
    let Some(stream_header) = frame_hub.stream_header() else {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    };
    let start_packets = &start_batch.batch.packets[start_batch.start_packet_index..];
    let Some(start_packet_bytes) = start_packets.first() else {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    };
    let mut last_sequence = start_batch.batch.sequence;
    if stream.write_all(&stream_header).is_err()
        || write_palette_cache_reset(&mut stream).is_err()
        || write_palette_packets(&mut stream, start_packets).is_err()
    {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    }

    stats.with_mut(|stats| {
        stats.latest_frame_bytes = start_packet_bytes.len();
        stats.custom_stage = "streaming";
    });

    loop {
        let Some(batch) = frame_hub.wait_for_delayed_encoded_batch_after(
            last_sequence,
            CUSTOM_STREAM_SERVER_DELAY,
            &stats,
            || active.is_active(),
        ) else {
            break;
        };
        if !active.is_active() || write_palette_batch(&mut stream, &batch.packets).is_err() {
            break;
        }
        last_sequence = batch.sequence;
        stats.with_mut(|stats| {
            stats.latest_frame_bytes = batch.bytes / batch.packets.len().max(1);
        });
    }
    stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
}

fn write_palette_batch(stream: &mut TcpStream, batch: &[Vec<u8>]) -> std::io::Result<()> {
    let batch_len: usize = 4 + batch.iter().map(|packet| 4 + packet.len()).sum::<usize>();
    let mut bytes = Vec::with_capacity(batch_len);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for packet in batch {
        bytes.extend_from_slice(&(packet.len() as u32).to_le_bytes());
        bytes.extend_from_slice(packet);
    }
    stream.write_all(&bytes)
}

fn write_palette_cache_reset(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(&0u32.to_le_bytes())
}

fn write_palette_packets(stream: &mut TcpStream, packets: &[Vec<u8>]) -> std::io::Result<()> {
    for packet in packets {
        let packet_len = packet.len() as u32;
        stream.write_all(&packet_len.to_le_bytes())?;
        stream.write_all(packet)?;
    }
    Ok(())
}

fn stream_pcm_audio(
    mut stream: TcpStream,
    audio: CustomAudioPacketHub,
    stats: SharedStats,
    active: CustomStreamState,
) {
    stats.with_mut(|stats| stats.stream_clients += 1);
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\nX-Audio-Format: mulaw8\r\nX-Audio-Sample-Rate: {CUSTOM_AUDIO_SAMPLE_RATE}\r\nX-Audio-Channels: {CUSTOM_AUDIO_CHANNELS}\r\n\r\n"
    );
    if stream.write_all(header.as_bytes()).is_err() {
        stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
        return;
    }

    let mut last_sequence = 0;
    loop {
        if !active.is_active() {
            break;
        }

        let Some((sequence, packet)) =
            audio.wait_for_packet_after_timeout(last_sequence, Duration::from_millis(100))
        else {
            continue;
        };
        last_sequence = sequence;

        if stream.write_all(&packet).is_err() {
            break;
        }
    }

    stats.with_mut(|stats| stats.stream_clients = stats.stream_clients.saturating_sub(1));
}

fn mjpeg_stream_page_html() -> String {
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
    header {{ display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }}
    button {{ appearance: none; border: 1px solid #4a5668; border-radius: 5px; background: #263142; color: #f8fafc; padding: 7px 10px; font: inherit; cursor: pointer; }}
    button:disabled {{ opacity: 0.55; cursor: default; }}
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

fn palette_stream_page_html() -> String {
    palette_stream_page_html_with_backend("")
}

pub fn static_palette_stream_page_html(backend_origin: &str) -> String {
    palette_stream_page_html_with_backend(backend_origin)
}

fn palette_stream_page_html_with_backend(backend_origin: &str) -> String {
    let backend = backend_origin.trim_end_matches('/');
    let palette_stream_url = format!("{backend}{PALETTE_STREAM_PATH}");
    let audio_stream_url = format!("{backend}{AUDIO_STREAM_PATH}");
    let local_chat_url = format!("{backend}{LOCAL_CHAT_PATH}");
    let stream_status_url = format!("{backend}{STREAM_STATUS_PATH}");
    let local_chat_feed_url = format!("{backend}{LOCAL_CHAT_FEED_PATH}");

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
    main {{ display: grid; place-items: center; padding: 16px; }}
    .stage {{ width: min(100%, 1240px); display: grid; grid-template-columns: auto minmax(220px, 320px); gap: 8px; align-items: stretch; justify-content: center; }}
    .player {{ position: relative; width: min(calc(100vw - 384px), calc(100vh - 84px), 960px); aspect-ratio: 1 / 1; }}
    canvas {{ display: block; width: 100%; height: 100%; object-fit: contain; image-rendering: pixelated; image-rendering: crisp-edges; background: #050608; border: 1px solid #303847; }}
    .unmute {{ position: absolute; inset: 1px; display: grid; place-items: center; border: 0; background: rgba(5, 6, 8, 0.54); color: #f8fafc; font: 700 clamp(18px, 4vw, 34px) Arial, sans-serif; cursor: pointer; text-shadow: 0 2px 8px #000; }}
    .unmute[hidden] {{ display: none; }}
    .player-controls {{ position: absolute; left: 10px; right: 10px; bottom: 10px; display: flex; gap: 8px; align-items: center; padding: 6px 8px; border-radius: 5px; background: rgba(5, 6, 8, 0.62); opacity: 0; pointer-events: none; transition: opacity 120ms ease; }}
    .player:hover .player-controls, .player-controls:focus-within {{ opacity: 1; pointer-events: auto; }}
    .player-controls[hidden] {{ display: none; }}
    .player-controls button {{ appearance: none; border: 1px solid #4a5668; border-radius: 4px; background: #263142; color: #f8fafc; padding: 4px 8px; font: inherit; cursor: pointer; }}
    .player-controls input {{ flex: 1; min-width: 0; }}
    .irc {{ min-height: 0; display: grid; grid-template-rows: auto 1fr auto; border: 1px solid #303847; background: #0b0d12; }}
    .irc h2 {{ margin: 0; padding: 10px 12px; border-bottom: 1px solid #303847; font-size: 14px; }}
    .irc-log {{ padding: 10px 12px; overflow: auto; color: #cbd5e1; font: 13px Consolas, monospace; }}
    .irc-log p {{ margin: 0 0 8px; }}
    .irc-input {{ display: flex; gap: 6px; padding: 10px; border-top: 1px solid #303847; }}
    .irc-input input {{ flex: 1; min-width: 0; background: #111722; color: #eef3f8; border: 1px solid #3a4353; border-radius: 4px; padding: 7px 8px; }}
    .irc-input button {{ border: 1px solid #4a5668; border-radius: 4px; background: #263142; color: #f8fafc; padding: 7px 10px; }}
    @media (max-width: 900px) {{ .stage {{ grid-template-columns: 1fr; }} .player {{ width: min(calc(100vw - 32px), calc(100vh - 280px), 960px); margin-inline: auto; }} .irc {{ min-height: 220px; }} }}
  </style>
</head>
<body>
  <header>Direct Stream Game custom palette stream</header>
  <main>
    <div class="stage">
      <div class="player">
        <canvas id="screen" width="{STREAM_WIDTH}" height="{STREAM_HEIGHT}"></canvas>
        <button class="unmute" id="unmuteButton" type="button">Click to unmute</button>
        <div class="player-controls" id="playerControls" hidden>
          <button id="muteButton" type="button">Mute</button>
          <input id="volumeSlider" type="range" min="0" max="1" step="0.01" value="0.8" aria-label="Volume">
        </div>
      </div>
      <aside class="irc">
        <h2>IRC</h2>
        <div class="irc-log" id="ircLog">
          <p><strong>system</strong> custom host chat panel ready</p>
        </div>
        <form class="irc-input" id="ircForm">
          <input id="ircInput" autocomplete="off" placeholder="chat message">
          <button type="submit">Send</button>
        </form>
      </aside>
    </div>
  </main>
  <script>
    const canvas = document.getElementById("screen");
    const player = document.querySelector(".player");
    const ctx = canvas.getContext("2d");
    const unmuteButton = document.getElementById("unmuteButton");
    const playerControls = document.getElementById("playerControls");
    const muteButton = document.getElementById("muteButton");
    const volumeSlider = document.getElementById("volumeSlider");
    const ircForm = document.getElementById("ircForm");
    const ircInput = document.getElementById("ircInput");
    const ircLog = document.getElementById("ircLog");
    ctx.imageSmoothingEnabled = false;

    let width = 0;
    let height = 0;
    let tileSize = 8;
    let palette = [];
    let tileCache = [];
    let framebuffer = new Uint8Array(0);
    let image = ctx.createImageData(1, 1);
    let tileScratch = new Uint8Array(64);
    let dirtyTileIndices = [];
    let pending = new Uint8Array(0);
    let frameQueue = [];
    let streamReady = false;
    let streamFps = 5;
    let frameIntervalMs = 1000 / streamFps;
    let playbackBufferSeconds = 1;
    let playbackRunning = false;
    let playbackStarted = false;
    let nextPlaybackAt = 0;
    let audioContext = null;
    let audioGain = null;
    let audioNode = null;
    let fallbackProcessor = null;
    let audioQueue = [];
    let audioQueueOffset = 0;
    let audioMuted = true;
    let audioStarted = false;
    let audioPrimed = false;
    let audioVolume = Number(volumeSlider.value);
    let audioShouldReconnect = false;
    let audioLoopRunning = false;
    let streamOnline = false;
    let lastChatId = 0;
    let chatGeneration = null;
    let shownChatIds = new Set();
    let lastAudioLeft = 0;
    let lastAudioRight = 0;
    let lastTransportSample = null;
    const audioTransportSampleRate = 8000;
    const audioPlaybackSampleRate = 48000;
    const audioUpsampleFactor = audioPlaybackSampleRate / audioTransportSampleRate;

    function resetStreamState() {{
      width = 0;
      height = 0;
      tileSize = 8;
      palette = [];
      tileCache = [];
      framebuffer = new Uint8Array(0);
      image = ctx.createImageData(1, 1);
      dirtyTileIndices.length = 0;
      pending = new Uint8Array(0);
      frameQueue = [];
      streamReady = false;
      playbackRunning = false;
      playbackStarted = false;
      nextPlaybackAt = 0;
    }}

    function setStreamTiming(fps, bufferSeconds) {{
      if (Number.isFinite(fps) && fps > 0) {{
        streamFps = Math.min(120, Math.max(1, fps));
        frameIntervalMs = 1000 / streamFps;
      }}
      if (Number.isFinite(bufferSeconds) && bufferSeconds >= 0) {{
        playbackBufferSeconds = Math.min(5, bufferSeconds);
      }}
    }}

    function appendBytes(a, b) {{
      const merged = new Uint8Array(a.length + b.length);
      merged.set(a, 0);
      merged.set(b, a.length);
      return merged;
    }}

    function readU32LE(bytes, offset) {{
      return bytes[offset] | (bytes[offset + 1] << 8) | (bytes[offset + 2] << 16) | (bytes[offset + 3] << 24);
    }}

    function readU16LE(bytes, offset) {{
      return bytes[offset] | (bytes[offset + 1] << 8);
    }}

    function readStreamHeader() {{
      if (streamReady) return true;
      if (pending.length < 12) return false;
      if (pending[0] !== 0x49 || pending[1] !== 0x50 || pending[2] !== 0x53 || pending[3] !== 0x43) {{
        pending = new Uint8Array(0);
        return false;
      }}

      width = readU16LE(pending, 5);
      height = readU16LE(pending, 7);
      tileSize = pending[9];
      const paletteLength = readU16LE(pending, 10);
      const headerLength = 12 + paletteLength * 4;
      if (pending.length < headerLength) return false;

      palette.length = 0;
      for (let i = 0; i < paletteLength; i++) {{
        const src = 12 + i * 4;
        palette.push([pending[src], pending[src + 1], pending[src + 2], pending[src + 3]]);
      }}

      canvas.width = width;
      canvas.height = height;
      canvas.style.aspectRatio = `${{width}} / ${{height}}`;
      player.style.aspectRatio = `${{width}} / ${{height}}`;
      framebuffer = new Uint8Array(width * height);
      image = ctx.createImageData(width, height);
      tileCache.length = 0;
      streamReady = true;
      pending = pending.slice(headerLength);
      return true;
    }}

    function consumeFrames() {{
      if (!readStreamHeader()) return;
      let offset = 0;
      while (pending.length - offset >= 4) {{
        const frameLength = readU32LE(pending, offset);
        if (frameLength === 0) {{
          frameQueue.push(null);
          offset += 4;
          continue;
        }}
        if (pending.length - offset - 4 < frameLength) {{
          break;
        }}

        const frame = pending.slice(offset + 4, offset + 4 + frameLength);
        frameQueue.push(frame);
        offset += 4 + frameLength;
      }}

      pending = pending.slice(offset);
      startPlaybackLoop();
    }}

    function startPlaybackLoop() {{
      if (playbackRunning) return;
      playbackRunning = true;
      nextPlaybackAt = performance.now();
      requestAnimationFrame(playbackTick);
    }}

    function playbackTick(now) {{
      if (!playbackRunning) return;
      if (!streamOnline || !streamReady) {{
        playbackRunning = false;
        playbackStarted = false;
        return;
      }}

      const bufferFrames = Math.max(1, Math.ceil(streamFps * playbackBufferSeconds));
      if (!playbackStarted) {{
        if (frameQueue.length < bufferFrames) {{
          requestAnimationFrame(playbackTick);
          return;
        }}
        playbackStarted = true;
        nextPlaybackAt = now;
      }}

      if (frameQueue.length === 0) {{
        playbackStarted = false;
        nextPlaybackAt = now + frameIntervalMs;
        requestAnimationFrame(playbackTick);
        return;
      }}

      while (frameQueue[0] === null) {{
        tileCache.length = 0;
        frameQueue.shift();
        if (frameQueue.length === 0) {{
          requestAnimationFrame(playbackTick);
          return;
        }}
      }}

      if (now >= nextPlaybackAt) {{
        drawFrame(frameQueue.shift());
        nextPlaybackAt += frameIntervalMs;
        if (now - nextPlaybackAt > frameIntervalMs * 4) {{
          nextPlaybackAt = now + frameIntervalMs;
        }}
      }}

      requestAnimationFrame(playbackTick);
    }}

    function drawFrame(frame) {{
      if (frame.length < 9) {{
        return;
      }}

      const frameType = frame[0];
      const payloadLength = readU32LE(frame, 5);
      const payload = frame.slice(9, 9 + payloadLength);

      if (frameType === 0) {{
        framebuffer.set(payload.slice(0, framebuffer.length));
        seedTileCacheFromFramebuffer();
        renderFramebuffer();
      }} else if (frameType === 1) {{
        applyDelta(payload);
        renderDirtyTiles();
      }} else {{
        return;
      }}
    }}

    function applyDelta(payload) {{
      const tilesX = width / tileSize;
      const tilesY = height / tileSize;
      const tileCount = tilesX * tilesY;
      const maskLength = Math.ceil(tileCount / 8);
      let cursor = maskLength;
      dirtyTileIndices.length = 0;

      for (let tileIndex = 0; tileIndex < tileCount; tileIndex++) {{
        if ((payload[Math.floor(tileIndex / 8)] & (1 << (tileIndex % 8))) === 0) {{
          continue;
        }}

        const tileX = tileIndex % tilesX;
        const tileY = Math.floor(tileIndex / tilesX);
        cursor = decodeTile(payload, cursor, tileX, tileY, tileScratch);
        writeTile(tileX, tileY, tileScratch);
        rememberTile(tileScratch);
        dirtyTileIndices.push(tileIndex);
      }}
    }}

    function decodeTile(bytes, cursor, tileX, tileY, tile) {{
      const mode = bytes[cursor++];
      readTile(tileX, tileY, tile);

      if (mode === 0) {{
        tile.set(bytes.subarray(cursor, cursor + 64));
        cursor += 64;
      }} else if (mode === 1) {{
        tile.fill(bytes[cursor++]);
      }} else if (mode === 2) {{
        cursor = decodeRle(tile, bytes, cursor, false);
      }} else if (mode === 3) {{
        const spanCount = bytes[cursor++];
        let out = 0;
        for (let span = 0; span < spanCount; span++) {{
          out += bytes[cursor++];
          const len = bytes[cursor++];
          tile.set(bytes.subarray(cursor, cursor + len), out);
          cursor += len;
          out += len;
        }}
      }} else if (mode === 4) {{
        cursor = decodeRle(tile, bytes, cursor, true);
      }} else if (mode === 5) {{
        const index = readU16LE(bytes, cursor);
        cursor += 2;
        if (tileCache[index]) {{
          tile.set(tileCache[index]);
        }}
      }}

      return cursor;
    }}

    function rememberTile(tile) {{
      if (tileCache.length >= 4096) return;
      tileCache.push(new Uint8Array(tile));
    }}

    function seedTileCacheFromFramebuffer() {{
      tileCache.length = 0;
      const tilesX = width / tileSize;
      const tilesY = height / tileSize;
      for (let tileY = 0; tileY < tilesY; tileY++) {{
        for (let tileX = 0; tileX < tilesX; tileX++) {{
          readTile(tileX, tileY, tileScratch);
          rememberTile(tileScratch);
        }}
      }}
    }}

    function decodeRle(tile, bytes, cursor, xor) {{
      const runCount = bytes[cursor++];
      let out = 0;
      for (let run = 0; run < runCount; run++) {{
        const value = bytes[cursor++];
        const len = bytes[cursor++];
        for (let i = 0; i < len; i++) {{
          tile[out] = xor ? tile[out] ^ value : value;
          out++;
        }}
      }}
      return cursor;
    }}

    function readTile(tileX, tileY, tile = new Uint8Array(64)) {{
      for (let y = 0; y < tileSize; y++) {{
        for (let x = 0; x < tileSize; x++) {{
          tile[y * tileSize + x] = framebuffer[(tileY * tileSize + y) * width + tileX * tileSize + x];
        }}
      }}
      return tile;
    }}

    function writeTile(tileX, tileY, tile) {{
      for (let y = 0; y < tileSize; y++) {{
        for (let x = 0; x < tileSize; x++) {{
          const pixelIndex = (tileY * tileSize + y) * width + tileX * tileSize + x;
          const paletteIndex = tile[y * tileSize + x];
          framebuffer[pixelIndex] = paletteIndex;
          const color = palette[paletteIndex] || palette[0] || [0, 0, 0, 255];
          const out = pixelIndex * 4;
          image.data[out] = color[0];
          image.data[out + 1] = color[1];
          image.data[out + 2] = color[2];
          image.data[out + 3] = color[3];
        }}
      }}
    }}

    function renderFramebuffer() {{
      for (let i = 0; i < framebuffer.length; i++) {{
        const color = palette[framebuffer[i]] || palette[0] || [0, 0, 0, 255];
        const out = i * 4;
        image.data[out] = color[0];
        image.data[out + 1] = color[1];
        image.data[out + 2] = color[2];
        image.data[out + 3] = color[3];
      }}
      ctx.putImageData(image, 0, 0);
    }}

    function renderDirtyTiles() {{
      for (let i = 0; i < dirtyTileIndices.length; i++) {{
        const tileIndex = dirtyTileIndices[i];
        const tileX = tileIndex % (width / tileSize);
        const tileY = Math.floor(tileIndex / (width / tileSize));
        ctx.putImageData(
          image,
          0,
          0,
          tileX * tileSize,
          tileY * tileSize,
          tileSize,
          tileSize
        );
      }}
    }}

    async function connect() {{
      while (true) {{
        try {{
          while (!streamOnline) {{
            await new Promise(resolve => setTimeout(resolve, 250));
          }}
          resetStreamState();
          const response = await fetch("{palette_stream_url}?t=" + Date.now(), {{ cache: "no-store" }});
          if (!response.ok) {{
            throw new Error(`stream failed: ${{response.status}}`);
          }}
          setStreamTiming(
            Number(response.headers.get("X-Stream-Fps")),
            Number(response.headers.get("X-Playback-Buffer-Seconds")),
          );
          const reader = response.body.getReader();
          while (true) {{
            const {{ value, done }} = await reader.read();
            if (done) break;
            pending = appendBytes(pending, value);
            consumeFrames();
            if (!streamOnline) break;
          }}
        }} catch (error) {{
          console.error(error);
        }}
        await new Promise(resolve => setTimeout(resolve, 500));
      }}
    }}

    function queuedAudioFrames() {{
      let frames = 0;
      for (let i = 0; i < audioQueue.length; i++) {{
        frames += audioQueue[i].length / 2;
      }}
      return Math.max(0, frames - audioQueueOffset);
    }}

    function dequeueAudioFrame() {{
      while (audioQueue.length > 0) {{
        const chunk = audioQueue[0];
        if (audioQueueOffset * 2 + 1 < chunk.length) {{
          const left = chunk[audioQueueOffset * 2];
          const right = chunk[audioQueueOffset * 2 + 1];
          audioQueueOffset++;
          if (audioQueueOffset * 2 >= chunk.length) {{
            audioQueue.shift();
            audioQueueOffset = 0;
          }}
          return [left, right];
        }}
        audioQueue.shift();
        audioQueueOffset = 0;
      }}
      return null;
    }}

    function enqueueAudioBytes(bytes) {{
      const samples = new Float32Array(bytes.byteLength * audioUpsampleFactor * 2);
      let out = 0;
      for (let i = 0; i < bytes.byteLength; i++) {{
        const current = mulawToLinear(bytes[i]);
        const previous = lastTransportSample === null ? current : lastTransportSample;
        for (let step = 0; step < audioUpsampleFactor; step++) {{
          const t = (step + 1) / audioUpsampleFactor;
          const sample = previous + (current - previous) * t;
          samples[out++] = sample;
          samples[out++] = sample;
        }}
        lastTransportSample = current;
      }}
      if (audioNode) {{
        audioNode.port.postMessage(samples, [samples.buffer]);
        return;
      }}
      audioQueue.push(samples);

      const maxBufferedFrames = 48000 * 2;
      while (queuedAudioFrames() > maxBufferedFrames && audioQueue.length > 1) {{
        audioQueue.shift();
        audioQueueOffset = 0;
      }}
    }}

    function mulawToLinear(byte) {{
      const magnitude = byte & 0x7f;
      const sign = (byte & 0x80) ? -1 : 1;
      const normalized = (Math.pow(256, magnitude / 127) - 1) / 255;
      return sign * normalized;
    }}

    async function createAudioNode() {{
      const workletSource = `
        class PcmStreamProcessor extends AudioWorkletProcessor {{
          constructor() {{
            super();
            this.queue = [];
            this.offset = 0;
            this.primed = false;
            this.lastLeft = 0;
            this.lastRight = 0;
            this.port.onmessage = event => this.queue.push(event.data);
          }}

          queuedFrames() {{
            let frames = 0;
            for (let i = 0; i < this.queue.length; i++) frames += this.queue[i].length / 2;
            return Math.max(0, frames - this.offset);
          }}

          nextFrame() {{
            while (this.queue.length > 0) {{
              const chunk = this.queue[0];
              if (this.offset * 2 + 1 < chunk.length) {{
                const left = chunk[this.offset * 2];
                const right = chunk[this.offset * 2 + 1];
                this.offset++;
                if (this.offset * 2 >= chunk.length) {{
                  this.queue.shift();
                  this.offset = 0;
                }}
                return [left, right];
              }}
              this.queue.shift();
              this.offset = 0;
            }}
            return null;
          }}

          process(inputs, outputs) {{
            const output = outputs[0];
            const left = output[0];
            const right = output[1] || output[0];
            if (!this.primed) {{
              if (this.queuedFrames() < 24000) {{
                left.fill(0);
                right.fill(0);
                return true;
              }}
              this.primed = true;
            }}

            for (let i = 0; i < left.length; i++) {{
              const frame = this.nextFrame();
              if (frame) {{
                this.lastLeft = frame[0];
                this.lastRight = frame[1];
                left[i] = frame[0];
                right[i] = frame[1];
              }} else {{
                const fade = 1 - i / left.length;
                left[i] = this.lastLeft * fade;
                right[i] = this.lastRight * fade;
                if (i === left.length - 1) {{
                  this.lastLeft = 0;
                  this.lastRight = 0;
                  this.primed = false;
                }}
              }}
            }}
            return true;
          }}
        }}
        registerProcessor("pcm-stream-processor", PcmStreamProcessor);
      `;
      const url = URL.createObjectURL(new Blob([workletSource], {{ type: "text/javascript" }}));
      try {{
        await audioContext.audioWorklet.addModule(url);
        const node = new AudioWorkletNode(audioContext, "pcm-stream-processor", {{
          numberOfOutputs: 1,
          outputChannelCount: [2],
        }});
        node.connect(audioGain);
        return node;
      }} finally {{
        URL.revokeObjectURL(url);
      }}
    }}

    function createFallbackAudioNode() {{
      const processor = audioContext.createScriptProcessor(4096, 0, 2);
      processor.onaudioprocess = event => {{
        const left = event.outputBuffer.getChannelData(0);
        const right = event.outputBuffer.getChannelData(1);
        if (!audioPrimed) {{
          if (queuedAudioFrames() < 48000 * 0.75) {{
            left.fill(0);
            right.fill(0);
            return;
          }}
          audioPrimed = true;
        }}
        for (let i = 0; i < left.length; i++) {{
          const frame = dequeueAudioFrame();
          if (frame) {{
            lastAudioLeft = frame[0];
            lastAudioRight = frame[1];
            left[i] = frame[0];
            right[i] = frame[1];
          }} else {{
            const fade = 1 - i / left.length;
            left[i] = lastAudioLeft * fade;
            right[i] = lastAudioRight * fade;
            if (i === left.length - 1) {{
              lastAudioLeft = 0;
              lastAudioRight = 0;
              audioPrimed = false;
            }}
          }}
        }}
      }};
      processor.connect(audioGain);
      return processor;
    }}

    function setMuted(muted) {{
      audioMuted = muted;
      if (audioGain && audioContext) {{
        const target = muted ? 0 : audioVolume;
        audioGain.gain.cancelScheduledValues(audioContext.currentTime);
        audioGain.gain.linearRampToValueAtTime(target, audioContext.currentTime + 0.025);
      }}
      unmuteButton.hidden = !muted;
      playerControls.hidden = muted;
      updateUnmuteOverlay();
    }}

    function updateUnmuteOverlay() {{
      if (!audioMuted) return;
      unmuteButton.textContent = streamOnline ? "Click to unmute" : "Not Online";
      unmuteButton.disabled = !streamOnline;
    }}

    async function pollStreamStatus() {{
      while (true) {{
        try {{
          const response = await fetch("{stream_status_url}?t=" + Date.now(), {{ cache: "no-store" }});
          const status = await response.json();
          streamOnline = !!status.online;
          updateUnmuteOverlay();
        }} catch (error) {{
          streamOnline = false;
          updateUnmuteOverlay();
        }}
        await new Promise(resolve => setTimeout(resolve, 500));
      }}
    }}

    async function runAudioStreamLoop() {{
      if (audioLoopRunning) return;
      audioLoopRunning = true;
      while (audioShouldReconnect) {{
        try {{
          const response = await fetch("{audio_stream_url}?t=" + Date.now(), {{ cache: "no-store" }});
          const reader = response.body.getReader();
          let receivedAudio = false;
          while (true) {{
            const {{ value, done }} = await reader.read();
            if (done) break;
            receivedAudio = true;
            enqueueAudioBytes(value);
          }}
          if (!receivedAudio) {{
            audioShouldReconnect = false;
            audioQueue = [];
            audioQueueOffset = 0;
            audioPrimed = false;
            lastTransportSample = null;
            setMuted(true);
            break;
          }}
        }} catch (error) {{
          console.error(error);
          audioPrimed = false;
        }}
        await new Promise(resolve => setTimeout(resolve, 500));
      }}
      audioLoopRunning = false;
    }}

    async function startAudio() {{
      if (audioContext) {{
        await audioContext.resume();
      }} else {{
        audioContext = new AudioContext({{ sampleRate: 48000 }});
        audioGain = audioContext.createGain();
        audioGain.gain.value = 0;
        try {{
          audioNode = await createAudioNode();
          console.info("Direct Stream audio: using AudioWorklet");
        }} catch (error) {{
          console.error(error);
          fallbackProcessor = createFallbackAudioNode();
          console.info("Direct Stream audio: using ScriptProcessor fallback");
        }}
        audioGain.connect(audioContext.destination);
        audioStarted = true;
      }}

      audioQueue = [];
      audioQueueOffset = 0;
      audioPrimed = false;
      lastTransportSample = null;
      audioShouldReconnect = true;
      setMuted(false);
      runAudioStreamLoop();
    }}

    unmuteButton.addEventListener("click", () => {{
      startAudio().catch(error => {{
        console.error(error);
        unmuteButton.hidden = false;
      }});
    }});

    muteButton.addEventListener("click", () => {{
      audioQueue = [];
      audioQueueOffset = 0;
      audioPrimed = false;
      lastTransportSample = null;
      setMuted(true);
    }});

    volumeSlider.addEventListener("input", () => {{
      audioVolume = Number(volumeSlider.value);
      if (audioStarted && !audioMuted) {{
        setMuted(false);
      }}
    }});

    ircForm.addEventListener("submit", event => {{
      event.preventDefault();
      const message = ircInput.value.trim();
      if (!message) return;
      ircInput.value = "";
      fetch("{local_chat_url}", {{
        method: "POST",
        headers: {{ "Content-Type": "text/plain;charset=utf-8" }},
        body: message,
        cache: "no-store",
      }})
        .then(response => response.ok ? response.json() : Promise.reject(new Error(`chat failed: ${{response.status}}`)))
        .then(() => fetchChatFeed())
        .catch(error => {{
          console.error(error);
          appendSystemLine("chat send failed");
        }});
    }});

    async function fetchChatFeed() {{
      const response = await fetch("{local_chat_feed_url}?after=" + lastChatId + "&t=" + Date.now(), {{ cache: "no-store" }});
      if (!response.ok) {{
        throw new Error(`chat feed failed: ${{response.status}}`);
      }}
      const feed = await response.json();
      if (Number.isFinite(feed.latest) && feed.latest < lastChatId) {{
        lastChatId = 0;
        shownChatIds.clear();
        clearChatLog();
      }}
      if (Number.isFinite(feed.generation) && chatGeneration !== feed.generation) {{
        chatGeneration = feed.generation;
        lastChatId = 0;
        shownChatIds.clear();
        clearChatLog();
      }}
      if (Array.isArray(feed.messages)) {{
        for (const message of feed.messages) {{
          if (shownChatIds.has(message.id)) continue;
          shownChatIds.add(message.id);
          appendChatLine(message.name || "unknown", message.text || "");
          lastChatId = Math.max(lastChatId, Number(message.id) || lastChatId);
        }}
      }}
      if (Number.isFinite(feed.latest)) {{
        lastChatId = Math.max(lastChatId, feed.latest);
      }}
    }}

    async function pollChatFeed() {{
      while (true) {{
        try {{
          await fetchChatFeed();
        }} catch (error) {{
          console.error(error);
          appendSystemLine("chat feed offline");
        }}
        await new Promise(resolve => setTimeout(resolve, 650));
      }}
    }}

    function appendSystemLine(message) {{
      const last = ircLog.lastElementChild;
      if (last && last.dataset.system === message) return;
      const row = document.createElement("p");
      row.dataset.system = message;
      const name = document.createElement("strong");
      name.textContent = "system";
      row.appendChild(name);
      row.append(" " + message);
      ircLog.appendChild(row);
      ircLog.scrollTop = ircLog.scrollHeight;
    }}

    function clearChatLog() {{
      ircLog.textContent = "";
      appendSystemLine("chat panel ready");
    }}

    function appendChatLine(displayName, message) {{
      const row = document.createElement("p");
      const name = document.createElement("strong");
      name.textContent = displayName;
      row.appendChild(name);
      row.append(" " + message);
      ircLog.appendChild(row);
      ircLog.scrollTop = ircLog.scrollHeight;
    }}

    connect();
    pollStreamStatus();
    pollChatFeed();
  </script>
</body>
</html>"#
    )
}
