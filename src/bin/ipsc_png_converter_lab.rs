use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

const ADDR: &str = "127.0.0.1:8093";
const DEFAULT_PALETTE_PATH: &str = "src/default_pallette/default_pallette.toml";

fn main() {
    let default_palette = fs::read_to_string(DEFAULT_PALETTE_PATH).unwrap_or_else(|err| {
        eprintln!("Could not load {DEFAULT_PALETTE_PATH}: {err}");
        String::new()
    });

    let listener = match TcpListener::bind(ADDR) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("Could not bind IPSI PNG converter lab at http://{ADDR}: {err}");
            return;
        }
    };

    eprintln!("IPSI PNG converter lab: http://{ADDR}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_request(stream, &default_palette),
            Err(err) => eprintln!("IPSI PNG converter lab connection failed: {err}"),
        }
    }
}

fn handle_request(mut stream: TcpStream, default_palette: &str) {
    let mut request = [0; 1024];
    let bytes_read = stream.read(&mut request).unwrap_or(0);
    let request = String::from_utf8_lossy(&request[..bytes_read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    match path {
        "/" => serve_page(stream),
        "/default_palette.toml" => serve_text(stream, default_palette, "text/plain; charset=utf-8"),
        _ => serve_not_found(stream),
    }
}

fn serve_page(mut stream: TcpStream) {
    let body = converter_html();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_text(mut stream: TcpStream, body: &str, content_type: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_not_found(mut stream: TcpStream) {
    let body = "not found";
    let response = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn converter_html() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>IPSI PNG Converter</title>
  <style>
    :root { color-scheme: dark; font-family: Arial, sans-serif; background: #101217; color: #edf2f7; }
    * { box-sizing: border-box; }
    body { margin: 0; min-height: 100vh; display: grid; grid-template-columns: minmax(330px, 410px) 1fr; }
    aside { border-right: 1px solid #2d3441; background: #171b23; padding: 16px; overflow-y: auto; }
    main { display: grid; grid-template-rows: auto 1fr; min-width: 0; }
    header { padding: 14px 16px; border-bottom: 1px solid #2d3441; display: flex; gap: 16px; align-items: center; flex-wrap: wrap; }
    h1 { font-size: 18px; margin: 0 0 16px; }
    fieldset { border: 1px solid #303847; border-radius: 6px; margin: 0 0 14px; padding: 12px; }
    legend { padding: 0 6px; color: #c4cede; font-weight: 700; }
    label { display: grid; grid-template-columns: 1fr 110px; gap: 12px; align-items: center; margin: 10px 0; }
    input[type="number"], select { width: 110px; padding: 8px; border: 1px solid #3a4557; border-radius: 5px; background: #0c0f15; color: #fff; font: inherit; }
    input[type="range"] { width: 110px; accent-color: #d8e8ff; }
    input[type="file"] { width: 100%; }
    button, .button { display: inline-grid; place-items: center; min-height: 40px; padding: 9px 12px; border: 1px solid #43516a; border-radius: 5px; background: #d8e8ff; color: #06101f; font: inherit; font-weight: 700; text-decoration: none; cursor: pointer; }
    button:disabled, .button[aria-disabled="true"] { opacity: 0.45; cursor: default; pointer-events: none; }
    .secondary { background: #263245; color: #f0f5ff; }
    .actions { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-top: 12px; }
    .dropzone { display: grid; gap: 8px; padding: 12px; border: 1px dashed #58677f; border-radius: 6px; background: #111720; }
    .dropzone.dragover { border-color: #bcd7ff; background: #172235; }
    .fileName { color: #b8c5d6; font-size: 13px; overflow-wrap: anywhere; min-height: 18px; }
    .previewWrap { padding: 18px; overflow: auto; display: grid; place-items: center; align-content: center; min-height: 0; }
    canvas { image-rendering: pixelated; image-rendering: crisp-edges; background: #050608; border: 1px solid #303847; width: min(92vmin, calc(100vw - 470px), calc(100vh - 120px)); min-width: 512px; height: auto; }
    pre { margin: 0; color: #b8ffd4; white-space: pre-wrap; font: 15px Consolas, monospace; }
    .hint { color: #aab7ca; font-size: 13px; line-height: 1.4; }
    @media (max-width: 820px) { body { grid-template-columns: 1fr; } aside { border-right: 0; border-bottom: 1px solid #2d3441; } canvas { width: min(94vw, 720px); min-width: 0; } }
  </style>
</head>
<body>
  <aside>
    <h1>IPSI PNG Converter</h1>
    <fieldset>
      <legend>Files</legend>
      <div class="dropzone" id="pngDrop">
        <strong>Input PNG</strong>
        <input id="pngFile" type="file" accept="image/png">
        <div class="fileName" id="pngName">No PNG selected</div>
      </div>
      <div style="height:10px"></div>
      <div class="dropzone" id="paletteDrop">
        <strong>Palette TOML</strong>
        <input id="paletteFile" type="file" accept=".toml,text/plain">
        <div class="fileName" id="paletteName">Using built-in default palette</div>
      </div>
    </fieldset>
    <fieldset>
      <legend>Output</legend>
      <label>width <input id="width" type="number" min="1" max="65535" value="128"></label>
      <label>height <input id="height" type="number" min="1" max="65535" value="128"></label>
      <label>dither <input id="dither" type="number" min="0" max="1" step="0.05" value="0.1"></label>
      <label>value <input id="valueAdjust" type="range" min="0.5" max="1.5" step="0.01" value="1"></label>
      <label>chroma <input id="chromaAdjust" type="range" min="0" max="2" step="0.01" value="1"></label>
      <label>downscale
        <select id="downscale">
          <option value="average">OKLCH average</option>
          <option value="majority">Majority</option>
          <option value="minority">Smallest minority</option>
          <option value="solve2x2">2x2 solver</option>
          <option value="solve2x2-hue">2x2 hue only</option>
        </select>
      </label>
    </fieldset>
    <button id="generate">Generate</button>
    <div class="actions">
      <a class="button secondary" id="downloadIpsi" aria-disabled="true">Download IPSI</a>
      <button class="secondary" id="resetPalette">Default Palette</button>
    </div>
    <p class="hint">The converter center-crops to the chosen aspect ratio, averages source hue/chroma/value in OKLab, dithers in OKLab, then writes one palette index per pixel.</p>
  </aside>
  <main>
    <header>
      <strong>Preview</strong>
      <pre id="status">Select a PNG, or drop one onto the file box.</pre>
    </header>
    <div class="previewWrap">
      <canvas id="preview" width="128" height="128"></canvas>
    </div>
  </main>
  <script>
    const pngFile = document.getElementById("pngFile");
    const paletteFile = document.getElementById("paletteFile");
    const pngName = document.getElementById("pngName");
    const paletteName = document.getElementById("paletteName");
    const generateButton = document.getElementById("generate");
    const downloadIpsi = document.getElementById("downloadIpsi");
    const resetPalette = document.getElementById("resetPalette");
    const status = document.getElementById("status");
    const canvas = document.getElementById("preview");
    const ctx = canvas.getContext("2d");
    ctx.imageSmoothingEnabled = false;

    let selectedPng = null;
    let paletteText = "";
    let paletteSource = "none";
    let customPaletteSelected = false;
    let ipsiUrl = null;

    const storedPalette = localStorage.getItem("ipscCurrentPaletteToml");
    if (storedPalette && parsePalette(storedPalette).length > 0) {
      usePaletteLabPalette(storedPalette, false);
    } else {
      fetch("/default_palette.toml", { cache: "no-store" })
        .then(response => response.text())
        .then(text => {
          if (customPaletteSelected) return;
          paletteText = text;
          paletteSource = "built-in default palette";
          status.textContent = "Default palette loaded. Select a PNG.";
        })
        .catch(error => status.textContent = `Could not load default palette: ${error}`);
    }

    pngFile.addEventListener("change", () => setPngFile(pngFile.files[0] || null));
    paletteFile.addEventListener("change", () => setPaletteFile(paletteFile.files[0] || null).catch(error => {
      console.error(error);
      status.textContent = error.toString();
    }));
    generateButton.addEventListener("click", () => generate().catch(error => {
      console.error(error);
      status.textContent = error.toString();
    }));
    resetPalette.addEventListener("click", async () => {
      const response = await fetch("/default_palette.toml", { cache: "no-store" });
      paletteText = await response.text();
      paletteSource = "built-in default palette";
      customPaletteSelected = false;
      localStorage.removeItem("ipscCurrentPaletteToml");
      localStorage.removeItem("ipscCurrentPaletteName");
      paletteFile.value = "";
      paletteName.textContent = "Using built-in default palette";
    });
    window.addEventListener("focus", () => refreshPaletteFromLab(false));
    window.addEventListener("pageshow", () => refreshPaletteFromLab(false));
    window.addEventListener("storage", event => {
      if (event.key === "ipscCurrentPaletteToml" || event.key === "ipscCurrentPaletteName") {
        refreshPaletteFromLab(true);
      }
    });

    setupDropzone(document.getElementById("pngDrop"), file => setPngFile(file), file => file.type === "image/png" || file.name.toLowerCase().endsWith(".png"));
    setupDropzone(document.getElementById("paletteDrop"), file => setPaletteFile(file).catch(error => {
      console.error(error);
      status.textContent = error.toString();
    }), file => file.name.toLowerCase().endsWith(".toml") || file.type.startsWith("text/"));

    function setupDropzone(element, accept, predicate) {
      element.addEventListener("dragover", event => {
        event.preventDefault();
        element.classList.add("dragover");
      });
      element.addEventListener("dragleave", () => element.classList.remove("dragover"));
      element.addEventListener("drop", event => {
        event.preventDefault();
        element.classList.remove("dragover");
        const file = [...event.dataTransfer.files].find(predicate);
        if (file) accept(file);
      });
    }

    function setPngFile(file) {
      selectedPng = file;
      pngName.textContent = file ? file.name : "No PNG selected";
    }

    async function setPaletteFile(file) {
      if (!file) return;
      const text = await file.text();
      const palette = parsePalette(text);
      if (palette.length === 0) {
        throw new Error(`${file.name} contains no #RRGGBB colours.`);
      }
      if (palette.length > 256) {
        throw new Error(`${file.name} contains more than 256 colours.`);
      }
      paletteText = text;
      paletteSource = file.name;
      customPaletteSelected = true;
      paletteName.textContent = `${file.name} (${palette.length} colours, ${paletteFingerprint(palette)})`;
      status.textContent = `Palette loaded: ${file.name}\n${palette.length} colours\n${paletteFingerprint(palette)}`;
    }

    async function generate() {
      if (!selectedPng) throw new Error("Select an input PNG first.");
      refreshPaletteFromLab(true);
      const palette = parsePalette(paletteText);
      if (palette.length === 0) throw new Error("Palette contains no #RRGGBB colours.");
      if (palette.length > 256) throw new Error("Palette contains more than 256 colours.");

      const width = parseDimension(document.getElementById("width").value, "width");
      const height = parseDimension(document.getElementById("height").value, "height");
      const ditherStrength = parseDither(document.getElementById("dither").value);
      const adjustments = {
        value: parseAdjustment(document.getElementById("valueAdjust").value, "value"),
        chroma: parseAdjustment(document.getElementById("chromaAdjust").value, "chroma"),
      };
      const source = await decodePng(selectedPng);
      const paletteOklab = palette.map(color => rgbToOklab(color[0], color[1], color[2]));
      const downscaleMode = document.getElementById("downscale").value;
      let prepared = downscaleMode === "majority" || downscaleMode === "minority"
        ? resizeToPaletteVote(source, width, height, palette, paletteOklab, downscaleMode, adjustments)
        : resizeToOklch(source, width, height, adjustments);
      if (downscaleMode === "solve2x2") prepared = solve2x2PaletteDownscale(prepared, paletteOklab);
      if (downscaleMode === "solve2x2-hue") prepared = solve2x2HueDownscale(prepared, paletteOklab);
      const pixels = quantize(prepared, palette, paletteOklab, ditherStrength);
      drawPreview(width, height, palette, pixels);
      const ipsi = makeIpsi(width, height, palette, pixels);
      setDownload(ipsi, outputName(selectedPng.name));
      const used = new Set(pixels).size;
      status.textContent = `${source.width}x${source.height} -> ${width}x${height}\npalette ${paletteSource}\n${palette.length} palette colours, ${used} used\n${paletteFingerprint(palette)}\ndither ${ditherStrength}\nvalue ${adjustments.value}, chroma ${adjustments.chroma}\ndownscale ${downscaleMode}`;
    }

    function refreshPaletteFromLab(announce) {
      const stored = localStorage.getItem("ipscCurrentPaletteToml");
      if (!stored) return false;
      const palette = parsePalette(stored);
      if (palette.length === 0) return false;
      if (stored === paletteText) return true;
      usePaletteLabPalette(stored, announce);
      return true;
    }

    function usePaletteLabPalette(text, announce) {
      const palette = parsePalette(text);
      paletteText = text;
      paletteSource = localStorage.getItem("ipscCurrentPaletteName") || "Palette Lab current palette";
      customPaletteSelected = true;
      paletteFile.value = "";
      paletteName.textContent = `${paletteSource} (${palette.length} colours, ${paletteFingerprint(palette)})`;
      if (announce) {
        status.textContent = `Using Palette Lab palette.\n${palette.length} colours\n${paletteFingerprint(palette)}`;
      }
    }

    function parseDimension(value, name) {
      const number = Number.parseInt(value, 10);
      if (!Number.isInteger(number) || number < 1 || number > 65535) throw new Error(`${name} must be 1..65535`);
      return number;
    }

    function parseDither(value) {
      const number = Number.parseFloat(value);
      if (!Number.isFinite(number) || number < 0 || number > 1) throw new Error("dither must be 0..1");
      return number;
    }

    function parseAdjustment(value, name) {
      const number = Number.parseFloat(value);
      if (!Number.isFinite(number)) throw new Error(`${name} must be a number`);
      return number;
    }

    function parsePalette(text) {
      const colors = [];
      for (const match of text.matchAll(/#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?/g)) {
        const hex = match[1];
        const alpha = match[2] || "ff";
        colors.push([
          Number.parseInt(hex.slice(0, 2), 16),
          Number.parseInt(hex.slice(2, 4), 16),
          Number.parseInt(hex.slice(4, 6), 16),
          Number.parseInt(alpha, 16),
        ]);
      }
      return colors;
    }

    function paletteFingerprint(palette) {
      const first = palette[0] ? colorHex(palette[0]) : "none";
      const middle = palette[Math.floor(palette.length / 2)] ? colorHex(palette[Math.floor(palette.length / 2)]) : "none";
      const last = palette[palette.length - 1] ? colorHex(palette[palette.length - 1]) : "none";
      return `first ${first}, middle ${middle}, last ${last}`;
    }

    function colorHex(color) {
      return String.fromCharCode(35) + color.slice(0, 3).map(value => value.toString(16).padStart(2, "0").toUpperCase()).join("");
    }

    async function decodePng(file) {
      const bitmap = await createImageBitmap(file);
      const work = document.createElement("canvas");
      work.width = bitmap.width;
      work.height = bitmap.height;
      const workCtx = work.getContext("2d", { willReadFrequently: true });
      workCtx.drawImage(bitmap, 0, 0);
      const data = workCtx.getImageData(0, 0, work.width, work.height).data;
      bitmap.close?.();
      return { width: work.width, height: work.height, data };
    }

    function resizeToOklch(source, width, height, adjustments) {
      const crop = cropBounds(source.width, source.height, width, height);
      const colors = new Array(width * height);
      const alpha = new Uint8Array(width * height);

      for (let y = 0; y < height; y++) {
        const sy0 = crop.y + Math.floor(y * crop.height / height);
        const sy1 = crop.y + Math.ceil((y + 1) * crop.height / height);
        for (let x = 0; x < width; x++) {
          const sx0 = crop.x + Math.floor(x * crop.width / width);
          const sx1 = crop.x + Math.ceil((x + 1) * crop.width / width);
          const sample = averageOklch(source, sx0, sy0, Math.max(sx1, sx0 + 1), Math.max(sy1, sy0 + 1), adjustments);
          const index = y * width + x;
          colors[index] = sample.color;
          alpha[index] = sample.alpha;
        }
      }

      return { width, height, colors, alpha };
    }

    function cropBounds(sourceWidth, sourceHeight, targetWidth, targetHeight) {
      const sourceAspect = sourceWidth * targetHeight;
      const targetAspect = targetWidth * sourceHeight;
      if (sourceAspect === targetAspect) return { x: 0, y: 0, width: sourceWidth, height: sourceHeight };
      if (sourceAspect > targetAspect) {
        const width = Math.max(1, Math.floor(sourceHeight * targetWidth / targetHeight));
        return { x: Math.floor((sourceWidth - width) / 2), y: 0, width, height: sourceHeight };
      }
      const height = Math.max(1, Math.floor(sourceWidth * targetHeight / targetWidth));
      return { x: 0, y: Math.floor((sourceHeight - height) / 2), width: sourceWidth, height };
    }

    function averageOklch(source, x0, y0, x1, y1, adjustments) {
      let lightness = 0;
      let chroma = 0;
      let hueX = 0;
      let hueY = 0;
      let alphaTotal = 0;
      let count = 0;
      for (let y = y0; y < Math.min(y1, source.height); y++) {
        for (let x = x0; x < Math.min(x1, source.width); x++) {
          const offset = (y * source.width + x) * 4;
          const alpha = source.data[offset + 3] / 255;
          if (alpha <= 0) {
            count++;
            continue;
          }
          const color = adjustOklch(rgbToOklab(source.data[offset], source.data[offset + 1], source.data[offset + 2]), adjustments);
          const c = chromaOf(color);
          const hue = Math.atan2(color.b, color.a);
          lightness += color.l * alpha;
          chroma += c * alpha;
          hueX += Math.cos(hue) * c * alpha;
          hueY += Math.sin(hue) * c * alpha;
          alphaTotal += alpha;
          count++;
        }
      }
      let color = { l: 0, a: 0, b: 0 };
      if (alphaTotal > 0) {
        const l = lightness / alphaTotal;
        const c = chroma / alphaTotal;
        if (Math.abs(hueX) + Math.abs(hueY) > 0.000001) {
          const hue = Math.atan2(hueY, hueX);
          color = { l, a: Math.cos(hue) * c, b: Math.sin(hue) * c };
        } else {
          color = { l, a: 0, b: 0 };
        }
      }
      return {
        color,
        alpha: count > 0 ? Math.round(alphaTotal / count * 255) : 0,
      };
    }

    function resizeToPaletteVote(source, width, height, palette, paletteOklab, voteMode, adjustments) {
      const crop = cropBounds(source.width, source.height, width, height);
      const colors = new Array(width * height);
      const alpha = new Uint8Array(width * height);

      for (let y = 0; y < height; y++) {
        const sy0 = crop.y + Math.floor(y * crop.height / height);
        const sy1 = crop.y + Math.ceil((y + 1) * crop.height / height);
        for (let x = 0; x < width; x++) {
          const sx0 = crop.x + Math.floor(x * crop.width / width);
          const sx1 = crop.x + Math.ceil((x + 1) * crop.width / width);
          const sample = voteOrPaletteAverage(source, sx0, sy0, Math.max(sx1, sx0 + 1), Math.max(sy1, sy0 + 1), palette, paletteOklab, voteMode, adjustments);
          const index = y * width + x;
          colors[index] = sample.color;
          alpha[index] = sample.alpha;
        }
      }

      return { width, height, colors, alpha };
    }

    function voteOrPaletteAverage(source, x0, y0, x1, y1, palette, paletteOklab, voteMode, adjustments) {
      const counts = new Array(Math.min(256, paletteOklab.length)).fill(0);
      const samples = [];
      let alphaTotal = 0;
      let count = 0;

      for (let y = y0; y < Math.min(y1, source.height); y++) {
        for (let x = x0; x < Math.min(x1, source.width); x++) {
          const offset = (y * source.width + x) * 4;
          const alpha = source.data[offset + 3] / 255;
          count++;
          if (alpha <= 0) continue;
          const sourceColor = adjustOklch(rgbToOklab(source.data[offset], source.data[offset + 1], source.data[offset + 2]), adjustments);
          const paletteIndex = nearestPaletteIndex(sourceColor, source.data[offset + 3], palette, paletteOklab);
          const color = paletteOklab[paletteIndex];
          if (!color) continue;
          counts[paletteIndex] += alpha;
          alphaTotal += alpha;
          samples.push([color, alpha]);
        }
      }

      if (alphaTotal <= 0) return { color: { l: 0, a: 0, b: 0 }, alpha: 0 };
      let bestIndex = -1;
      let bestCount = voteMode === "minority" ? Number.POSITIVE_INFINITY : -1;
      for (let i = 0; i < counts.length; i++) {
        if (counts[i] <= 0) continue;
        if ((voteMode === "minority" && counts[i] < bestCount) || (voteMode !== "minority" && counts[i] > bestCount)) {
          bestIndex = i;
          bestCount = counts[i];
        }
      }
      const color = bestIndex >= 0
        ? paletteOklab[bestIndex]
        : weightedAverageOklch(samples);
      return {
        color,
        alpha: count > 0 ? Math.round(alphaTotal / count * 255) : 0,
      };
    }

    function weightedAverageOklch(samples) {
      let lightness = 0;
      let chroma = 0;
      let hueX = 0;
      let hueY = 0;
      let total = 0;
      for (const [color, weight] of samples) {
        const c = chromaOf(color);
        const hue = Math.atan2(color.b, color.a);
        lightness += color.l * weight;
        chroma += c * weight;
        hueX += Math.cos(hue) * c * weight;
        hueY += Math.sin(hue) * c * weight;
        total += weight;
      }
      if (total <= 0) return { l: 0, a: 0, b: 0 };
      const l = lightness / total;
      const c = chroma / total;
      if (Math.abs(hueX) + Math.abs(hueY) > 0.000001) {
        const hue = Math.atan2(hueY, hueX);
        return { l, a: Math.cos(hue) * c, b: Math.sin(hue) * c };
      }
      return { l, a: 0, b: 0 };
    }

    function quantize(image, palette, paletteOklab, ditherStrength) {
      const colors = image.colors.map(color => ({ ...color }));
      const pixels = new Uint8Array(image.width * image.height);
      for (let y = 0; y < image.height; y++) {
        for (let x = 0; x < image.width; x++) {
          const index = y * image.width + x;
          const color = clampOklab(colors[index]);
          const paletteIndex = nearestPaletteIndex(color, image.alpha[index], palette, paletteOklab);
          pixels[index] = paletteIndex;

          if (ditherStrength <= 0 || image.alpha[index] === 0) continue;
          const quantized = paletteOklab[paletteIndex] || color;
          const error = { l: color.l - quantized.l, a: color.a - quantized.a, b: color.b - quantized.b };
          diffuse(colors, image.width, image.height, x + 1, y, error, 7 / 16 * ditherStrength);
          if (x > 0) diffuse(colors, image.width, image.height, x - 1, y + 1, error, 3 / 16 * ditherStrength);
          diffuse(colors, image.width, image.height, x, y + 1, error, 5 / 16 * ditherStrength);
          diffuse(colors, image.width, image.height, x + 1, y + 1, error, 1 / 16 * ditherStrength);
        }
      }
      return pixels;
    }

    function solve2x2PaletteDownscale(image, paletteOklab) {
      if (image.width < 2 || image.height < 2 || paletteOklab.length === 0) return image;
      const candidateSets = Array.from({ length: image.width * image.height }, () => []);
      for (let y = 0; y < image.height - 1; y++) {
        for (let x = 0; x < image.width - 1; x++) {
          const indices = [
            y * image.width + x,
            y * image.width + x + 1,
            (y + 1) * image.width + x,
            (y + 1) * image.width + x + 1,
          ];
          const target = averageOklchValues(indices.map(index => image.colors[index]));
          const solved = bestPaletteQuad(target, paletteOklab);
          for (const index of indices) candidateSets[index].push(...solved);
        }
      }
      return {
        width: image.width,
        height: image.height,
        alpha: image.alpha,
        colors: candidateSets.map((candidates, index) => candidates.length ? averageOklchValues(candidates) : image.colors[index]),
      };
    }

    function solve2x2HueDownscale(image, paletteOklab) {
      if (image.width < 2 || image.height < 2 || paletteOklab.length === 0) return image;
      const hueSets = Array.from({ length: image.width * image.height }, () => []);
      for (let y = 0; y < image.height - 1; y++) {
        for (let x = 0; x < image.width - 1; x++) {
          const indices = [
            y * image.width + x,
            y * image.width + x + 1,
            (y + 1) * image.width + x,
            (y + 1) * image.width + x + 1,
          ];
          const target = averageOklchValues(indices.map(index => image.colors[index]));
          const solved = bestPaletteQuad(target, paletteOklab);
          for (const index of indices) hueSets[index].push(...solved);
        }
      }
      return {
        width: image.width,
        height: image.height,
        alpha: image.alpha,
        colors: hueSets.map((candidates, index) => {
          const base = image.colors[index];
          const chroma = chromaOf(base);
          const hue = averageHue(candidates);
          if (chroma <= 0.000001 || hue === null) return base;
          return { l: base.l, a: Math.cos(hue) * chroma, b: Math.sin(hue) * chroma };
        }),
      };
    }

    function bestPaletteQuad(target, paletteOklab) {
      const candidates = [...paletteOklab]
        .slice(0, 256)
        .sort((left, right) => distanceSquared(left, target) - distanceSquared(right, target))
        .slice(0, Math.min(10, paletteOklab.length));
      let best = [candidates[0], candidates[0], candidates[0], candidates[0]];
      let bestDistance = Number.POSITIVE_INFINITY;
      for (let a = 0; a < candidates.length; a++) {
        for (let b = a; b < candidates.length; b++) {
          for (let c = b; c < candidates.length; c++) {
            for (let d = c; d < candidates.length; d++) {
              const quad = [candidates[a], candidates[b], candidates[c], candidates[d]];
              const average = averageOklchValues(quad);
              const distance = distanceSquared(average, target);
              if (distance < bestDistance) {
                bestDistance = distance;
                best = quad;
              }
            }
          }
        }
      }
      return best;
    }

    function averageOklchValues(values) {
      let lightness = 0;
      let chroma = 0;
      let hueX = 0;
      let hueY = 0;
      for (const color of values) {
        const c = chromaOf(color);
        const hue = Math.atan2(color.b, color.a);
        lightness += color.l;
        chroma += c;
        hueX += Math.cos(hue) * c;
        hueY += Math.sin(hue) * c;
      }
      if (values.length === 0) return { l: 0, a: 0, b: 0 };
      const l = lightness / values.length;
      const c = chroma / values.length;
      if (Math.abs(hueX) + Math.abs(hueY) > 0.000001) {
        const hue = Math.atan2(hueY, hueX);
        return { l, a: Math.cos(hue) * c, b: Math.sin(hue) * c };
      }
      return { l, a: 0, b: 0 };
    }

    function averageHue(values) {
      let hueX = 0;
      let hueY = 0;
      for (const color of values) {
        const c = chromaOf(color);
        const hue = Math.atan2(color.b, color.a);
        hueX += Math.cos(hue) * c;
        hueY += Math.sin(hue) * c;
      }
      return Math.abs(hueX) + Math.abs(hueY) > 0.000001 ? Math.atan2(hueY, hueX) : null;
    }

    function nearestPaletteIndex(color, alpha, palette, paletteOklab) {
      if (alpha === 0) {
        const transparent = palette.findIndex(color => color[3] === 0);
        return transparent >= 0 ? transparent : 0;
      }
      let best = 0;
      let bestDistance = Number.POSITIVE_INFINITY;
      for (let i = 0; i < Math.min(256, paletteOklab.length); i++) {
        const candidate = paletteOklab[i];
        const alphaDistance = ((alpha - palette[i][3]) / 255) ** 2;
        const distance =
          (color.l - candidate.l) ** 2 +
          (color.a - candidate.a) ** 2 +
          (color.b - candidate.b) ** 2 +
          alphaDistance;
        if (distance < bestDistance) {
          bestDistance = distance;
          best = i;
        }
      }
      return best;
    }

    function diffuse(colors, width, height, x, y, error, factor) {
      if (x < 0 || y < 0 || x >= width || y >= height) return;
      const color = colors[y * width + x];
      color.l += error.l * factor;
      color.a += error.a * factor;
      color.b += error.b * factor;
    }

    function clampOklab(color) {
      return {
        l: Math.min(1, Math.max(0, color.l)),
        a: Math.min(0.5, Math.max(-0.5, color.a)),
        b: Math.min(0.5, Math.max(-0.5, color.b)),
      };
    }

    function adjustOklch(color, adjustments) {
      const chroma = chromaOf(color) * adjustments.chroma;
      const hue = Math.atan2(color.b, color.a);
      return {
        l: Math.min(1, Math.max(0, color.l * adjustments.value)),
        a: Math.cos(hue) * chroma,
        b: Math.sin(hue) * chroma,
      };
    }

    function chromaOf(color) {
      return Math.hypot(color.a, color.b);
    }

    function distanceSquared(left, right) {
      return (left.l - right.l) ** 2 + (left.a - right.a) ** 2 + (left.b - right.b) ** 2;
    }

    function rgbToOklab(r8, g8, b8) {
      const r = srgbToLinear(r8 / 255);
      const g = srgbToLinear(g8 / 255);
      const b = srgbToLinear(b8 / 255);
      const l = 0.41222146 * r + 0.53633255 * g + 0.051445995 * b;
      const m = 0.2119035 * r + 0.6806995 * g + 0.10739696 * b;
      const s = 0.08830246 * r + 0.28171884 * g + 0.6299787 * b;
      const l_ = Math.cbrt(l);
      const m_ = Math.cbrt(m);
      const s_ = Math.cbrt(s);
      return {
        l: 0.21045426 * l_ + 0.7936178 * m_ - 0.004072047 * s_,
        a: 1.9779985 * l_ - 2.4285922 * m_ + 0.4505937 * s_,
        b: 0.025904037 * l_ + 0.78277177 * m_ - 0.80867577 * s_,
      };
    }

    function srgbToLinear(value) {
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    }

    function drawPreview(width, height, palette, pixels) {
      canvas.width = width;
      canvas.height = height;
      const image = ctx.createImageData(width, height);
      for (let i = 0; i < pixels.length; i++) {
        const color = palette[pixels[i]] || palette[0] || [0, 0, 0, 255];
        const out = i * 4;
        image.data[out] = color[0];
        image.data[out + 1] = color[1];
        image.data[out + 2] = color[2];
        image.data[out + 3] = color[3];
      }
      ctx.putImageData(image, 0, 0);
    }

    function makeIpsi(width, height, palette, pixels) {
      const bytes = new Uint8Array(11 + palette.length * 4 + pixels.length);
      let cursor = 0;
      bytes.set([0x49, 0x50, 0x53, 0x49], cursor); cursor += 4;
      bytes[cursor++] = 1;
      writeU16(bytes, cursor, width); cursor += 2;
      writeU16(bytes, cursor, height); cursor += 2;
      writeU16(bytes, cursor, palette.length); cursor += 2;
      for (const color of palette) {
        bytes[cursor++] = color[0];
        bytes[cursor++] = color[1];
        bytes[cursor++] = color[2];
        bytes[cursor++] = color[3];
      }
      bytes.set(pixels, cursor);
      return bytes;
    }

    function writeU16(bytes, offset, value) {
      bytes[offset] = value & 0xff;
      bytes[offset + 1] = (value >> 8) & 0xff;
    }

    function setDownload(bytes, name) {
      if (ipsiUrl) URL.revokeObjectURL(ipsiUrl);
      ipsiUrl = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
      downloadIpsi.href = ipsiUrl;
      downloadIpsi.download = name;
      downloadIpsi.setAttribute("aria-disabled", "false");
    }

    function outputName(inputName) {
      return inputName.replace(/\.[^.]+$/, "") + ".ipsi";
    }
  </script>
</body>
</html>"#
    .to_owned()
}
