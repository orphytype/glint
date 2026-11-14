//! Renders one of the built-in demo tracks to a WAV file you can listen to, and draws its
//! loudness over time so the beat and the arrangement are visible in the terminal.
//!
//! ```text
//! cargo run -p ym-audio --example render_demo -- house 7 40 house.wav
//!                                                 ^style ^seed ^seconds ^output (optional)
//! ```

use ym_audio::synth::{self, RATE, Style};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let style = args.first().and_then(|s| Style::ALL.into_iter().find(|st| st.name() == s)).unwrap_or(Style::Synthwave);
    let seed: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let secs: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30.0);

    let started = std::time::Instant::now();
    let samples = synth::render(seed, style, secs);
    println!("{} (seed {seed}), {secs:.0} s, rendered in {:.2?}", style.name(), started.elapsed());

    // Loudness in 50 ms steps, drawn as bars (first 12 seconds are enough to see the groove).
    let step = RATE as usize / 20 * 2;
    println!("\nloudness, one row per 50 ms:");
    for (i, chunk) in samples.chunks(step).take(240).enumerate() {
        let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
        let bar = "█".repeat((rms * 140.0) as usize);
        println!("{:>5.2}s {bar}", i as f32 * 0.05);
    }

    if let Some(path) = args.get(3) {
        std::fs::write(path, ym_audio::wav::encode(&samples, 2, RATE)).expect("write wav");
        println!("\nwrote {path}");
    } else {
        println!("\n(pass a fourth argument to save a WAV: ... {} {seed} {secs:.0} out.wav)", style.name());
    }
}
