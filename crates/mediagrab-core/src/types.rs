use std::path::PathBuf;

/// What the user wants to get out of the link(s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Audio only. Title is written into the file's tags and the thumbnail is
    /// embedded as cover art.
    Audio,
    /// Video (mp4). Title only: no thumbnail is downloaded or embedded.
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    M4a,
    Opus,
    Flac,
}

impl AudioFormat {
    pub fn as_arg(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::M4a => "m4a",
            AudioFormat::Opus => "opus",
            AudioFormat::Flac => "flac",
        }
    }
    pub const ALL: [AudioFormat; 4] = [
        AudioFormat::Mp3,
        AudioFormat::M4a,
        AudioFormat::Opus,
        AudioFormat::Flac,
    ];
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Where files are written. Playlists get their own sub-folder.
    pub out_dir: PathBuf,
    pub mode: Mode,
    pub audio_format: AudioFormat,
    /// Video only: cap the resolution (e.g. Some(1080)). None = best available.
    pub max_height: Option<u32>,
    /// How many downloads run at the same time.
    pub parallel: usize,
    /// Audio only: ALSO save the thumbnail as a separate .jpg next to the audio
    /// (it is always embedded as cover art regardless).
    pub save_cover_file: bool,
    /// Name or path of the yt-dlp executable.
    pub ytdlp: String,
    /// Extra yt-dlp arguments appended to every call, e.g.
    /// `--js-runtimes node` or `--cookies-from-browser firefox`.
    pub extra_args: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("downloads"),
            mode: Mode::Audio,
            audio_format: AudioFormat::Mp3,
            max_height: Some(1080),
            parallel: 4,
            save_cover_file: false,
            ytdlp: "yt-dlp".to_string(),
            extra_args: Vec::new(),
        }
    }
}

/// One downloadable thing (a single video or a single track).
#[derive(Clone, Debug)]
pub struct Item {
    /// Unique within one batch. The UI uses it to find the row to update.
    pub id: usize,
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    /// Set when this item came from a playlist/set with more than one entry.
    pub playlist: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemState {
    Queued,
    Downloading,
    /// Download finished; yt-dlp/ffmpeg is converting, merging or tagging.
    Processing,
    Done(PathBuf),
    Failed(String),
    Cancelled,
}

/// Everything the engine tells the outside world. The UI just consumes these.
#[derive(Clone, Debug)]
pub enum Event {
    Status(String),
    ItemsFound(Vec<Item>),
    Progress {
        id: usize,
        percent: Option<f32>,
        speed: Option<String>,
        eta: Option<String>,
    },
    State {
        id: usize,
        state: ItemState,
    },
    /// A problem that is not tied to one item (e.g. a link that could not be resolved).
    Error(String),
    /// Everything is done (or cancelled). No more events follow.
    Finished,
}
