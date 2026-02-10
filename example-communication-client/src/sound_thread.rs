use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::LazyLock;
use std::thread;
use lofty::file::AudioFile;
use rodio::{OutputStream, OutputStreamBuilder, Sink};
use lofty::probe::Probe;
use lofty::read_from_path;

static STREAM_HANDLE: LazyLock<OutputStream> = LazyLock::new(|| {
    OutputStreamBuilder::open_default_stream().expect("Failed to open default audio stream")
});

pub fn spawn_sound_thread(path: String) {
    println!("Attempted to play: {}", path);
    thread::spawn(move || {
        let parsed_path = Path::new(&path);
        let tagged_file = read_from_path(parsed_path);
        let mut duration = std::time::Duration::from_secs(5);
        if tagged_file.is_ok()
        {
            let stage_one = Probe::open(parsed_path);
            let stage_two = stage_one.unwrap().guess_file_type();
            let stage_three = stage_two.unwrap().read();
            let probed_file = stage_three.unwrap();
            duration = probed_file.properties().duration();
        }

        let audio_file = BufReader::new(File::open(path).unwrap());
        let _sink: Sink;
        {
            _sink = rodio::play(&STREAM_HANDLE.mixer(), audio_file).unwrap();
        }

        thread::sleep(duration);
    });
}