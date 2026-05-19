use direct_stream_game::static_palette_stream_page_html;
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), String> {
    let backend = env::args()
        .nth(1)
        .unwrap_or_else(|| "https://game.humanbeangames.com".to_owned());
    let out_dir = PathBuf::from("dist/humanbeangames_stream");
    fs::create_dir_all(&out_dir).map_err(|err| err.to_string())?;
    fs::write(
        out_dir.join("index.html"),
        static_palette_stream_page_html(&backend),
    )
    .map_err(|err| err.to_string())?;

    println!(
        "Exported stream player to {} using backend {}",
        out_dir.display(),
        backend
    );
    Ok(())
}
