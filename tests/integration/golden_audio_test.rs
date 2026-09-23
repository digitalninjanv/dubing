use audiodub::domain::AudioFormat;
use audiodub::infrastructure::ffmpeg::{FfmpegExporter, FfprobeInspector};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct Manifest {
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    duration_ms: u64,
    sample_rate_hz: u32,
    channels: u32,
    max_duration_error_ms: u64,
    min_output_bytes: u64,
}

fn generate_fixture(path: &Path, fixture: &Fixture) {
    let layout = if fixture.channels == 2 {
        "stereo"
    } else {
        "mono"
    };
    let duration = fixture.duration_ms as f64 / 1000.0;
    let status = Command::new("ffmpeg")
        .args(["-y", "-f", "lavfi"])
        .arg("-i")
        .arg(format!(
            "sine=frequency=440:sample_rate={}:duration={}",
            fixture.sample_rate_hz, duration
        ))
        .args(["-ac", if fixture.channels == 2 { "2" } else { "1" }])
        .args(["-channel_layout", layout])
        .arg(path)
        .status()
        .expect("ffmpeg must be installed for golden audio benchmark");
    assert!(
        status.success(),
        "failed to create fixture {}",
        fixture.name
    );
}

#[test]
fn golden_audio_export_stays_within_duration_and_integrity_tolerance() {
    let manifest: Manifest =
        serde_json::from_str(include_str!("../golden_audio.json")).expect("valid golden manifest");

    for fixture in &manifest.fixtures {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join(format!("{}_input.wav", fixture.name));
        let output = dir.path().join(format!("{}_output.mp3", fixture.name));

        generate_fixture(&input, fixture);

        let artifact = FfmpegExporter::export(
            &[input.clone()],
            &output,
            AudioFormat::Mp3,
            192,
            Vec::new(),
            Some(fixture.duration_ms),
        )
        .expect("golden export must succeed");

        assert!(output.is_file(), "{} output missing", fixture.name);
        assert!(
            artifact.size_bytes >= fixture.min_output_bytes,
            "{} output unexpectedly small",
            fixture.name
        );

        let metadata = FfprobeInspector::probe(&output).expect("golden output must be probeable");
        let duration_error = metadata.duration_ms.abs_diff(fixture.duration_ms);
        assert!(
            duration_error <= fixture.max_duration_error_ms,
            "{} duration drift {}ms exceeds {}ms",
            fixture.name,
            duration_error,
            fixture.max_duration_error_ms
        );
        assert!(metadata.duration_ms > 0);
    }
}
