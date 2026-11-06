//! Sample data for looking at the UI without an account. Debug builds only
//! (`PUTIO_ADDER_DEMO=1 cargo run -p putio-adder`): this module does not exist in release builds.

use putio_api::{Account, Disk, File, FileType, RssFeed, Transfer, TransferKind, TransferStatus};

fn transfer(
    id: i64,
    name: &str,
    status: TransferStatus,
    percent: i64,
    speed: i64,
    eta: Option<i64>,
    size: i64,
) -> Transfer {
    Transfer {
        id,
        name: name.into(),
        status,
        kind: TransferKind::Torrent,
        percent_done: percent,
        down_speed: speed,
        up_speed: 0,
        estimated_time: eta,
        size,
        file_id: matches!(status, TransferStatus::Completed | TransferStatus::Seeding)
            .then_some(1000 + id),
        error_message: None,
        source: None,
        ..Transfer::default()
    }
}

pub(crate) fn transfers() -> Vec<Transfer> {
    use TransferStatus::{Completed, Downloading, Error, InQueue, Seeding, Waiting};
    const MB: i64 = 1024 * 1024;
    let mut failed = transfer(
        7,
        "Corrupted.Release.2024.1080p.WEB-DL",
        Error,
        3,
        0,
        None,
        4_200 * MB,
    );
    failed.error_message = Some("Tracker unreachable".into());
    failed.source = Some("https://example.com/releases/corrupted.torrent".into());
    let mut list = vec![
        transfer(
            1,
            "Big.Buck.Bunny.2008.2160p.BluRay.x265-GROUP",
            Downloading,
            42,
            6 * MB + MB / 2,
            Some(247),
            3_400 * MB,
        ),
        transfer(
            2,
            "Sintel.2010.1080p.BluRay.x264-GROUP",
            Downloading,
            87,
            2 * MB,
            Some(35),
            1_100 * MB,
        ),
        transfer(3, "Tears.of.Steel.2012.720p", Waiting, 0, 0, None, 800 * MB),
        transfer(
            4,
            "Elephants.Dream.2006.1080p.BluRay",
            InQueue,
            0,
            0,
            None,
            1_500 * MB,
        ),
        transfer(
            5,
            "Cosmos.Laundromat.2015.1080p",
            Completed,
            100,
            0,
            None,
            950 * MB,
        ),
        transfer(
            6,
            "Spring.2019.Blender.Open.Movie.2160p",
            Seeding,
            100,
            0,
            None,
            4_800 * MB,
        ),
        failed,
    ];
    add_details(&mut list);
    list
}

/// A few feeds in different states, newest first like the real list: running, filtered, paused, one put.io
/// cannot read, one without a name, and one with a name too long for its card.
pub(crate) fn feeds() -> Vec<RssFeed> {
    let feed = |id: i64, title: &str, url: &str, keyword: &str| RssFeed {
        id,
        title: title.into(),
        rss_source_url: url.into(),
        parent_dir_id: Some(12),
        keyword: (!keyword.is_empty()).then(|| keyword.to_owned()),
        ..RssFeed::default()
    };
    vec![
        feed(
            6,
            "Weekly releases",
            "https://releases.example.invalid/rss/weekly.xml?key=SECRET",
            "1080p, 2160p",
        ),
        feed(5, "", "https://feeds.example.invalid/all.xml", ""),
        RssFeed {
            paused: true,
            ..feed(
                4,
                "Open-source ISOs",
                "https://downloads.example.invalid/isos.rss",
                "",
            )
        },
        RssFeed {
            last_error: Some("FEED_CANNOT_FETCHED".into()),
            ..feed(
                3,
                "Podcast (moved, address no longer works)",
                "https://old.example.invalid/podcast.xml",
                "",
            )
        },
        feed(
            2,
            "A feed with a very long name that keeps going and going so that it has to be cut to fit",
            "https://a-really-long-host-name-for-a-feed.subdomain.example.invalid/some/path.xml",
            "uncompressed, remux, hdr10plus, dolby vision, atmos, truehd, 7.1",
        ),
        feed(1, "Short", "https://x.example.invalid/f", "hd"),
    ]
}

/// What is in the top folder of the demo account: folders and files of every kind, one with a name too long
/// for its row (the listing is not sorted here: the core does that).
pub(crate) fn files() -> Vec<File> {
    let item = |id: i64, name: &str, kind: FileType, size: i64| File {
        id,
        name: name.into(),
        parent_id: Some(0),
        file_type: kind,
        content_type: String::new(),
        size,
        screenshot: None,
        created_at: Some(format!("2024-0{}-1{}T12:00:00", 1 + id % 9, id % 9)),
    };
    vec![
        item(11, "Movies", FileType::Folder, 0),
        item(12, "TV Shows", FileType::Folder, 0),
        item(13, "Music", FileType::Folder, 0),
        item(
            14,
            "Sintel.2010.1080p.BluRay.x264-GROUP.mkv",
            FileType::Video,
            1_100_000_000,
        ),
        item(
            15,
            "Cosmos.Laundromat.2015.flac",
            FileType::Audio,
            95_000_000,
        ),
        item(16, "holiday-photo.jpg", FileType::Image, 3_400_000),
        item(
            17,
            "Big.Buck.Bunny.2008.2160p.zip",
            FileType::Archive,
            3_300_000_000,
        ),
        item(18, "manual.pdf", FileType::Pdf, 1_200_000),
        item(19, "notes.txt", FileType::Text, 4_000),
        item(20, "something-unknown.bin", FileType::Unknown, 12_000_000),
        item(
            21,
            "A.file.with.a.very.long.name.that.keeps.going.and.going.so.that.it.has.to.be.cut.to.fit.in.its.row.2160p.UHD.mkv",
            FileType::Video,
            9_800_000_000,
        ),
        item(
            22,
            "Neue Datei mit Umlauten äöü.mkv",
            FileType::Video,
            700_000_000,
        ),
    ]
}

pub(crate) fn account() -> Account {
    Account {
        user_id: 1,
        username: "nico".into(),
        mail: String::new(),
        monthly_bandwidth_usage: 7_700_000_000,
        disk: Some(Disk {
            size: 1_000_000_000_000,
            used: 340_000_000_000,
            avail: 660_000_000_000,
        }),
    }
}

/// Peers, ratios, tracker messages and dates, so the details panel has something to show.
/// The folders the demo transfers save into (their names are in [`folder_names`]).
const DEMO_MOVIES: i64 = 11;
const DEMO_SHOWS: i64 = 12;

/// What put.io would say when asked for the names of those folders.
pub(crate) fn folder_names() -> [(i64, &'static str); 2] {
    [(DEMO_MOVIES, "Movies"), (DEMO_SHOWS, "TV Shows")]
}

fn add_details(list: &mut [Transfer]) {
    for item in list {
        match item.id {
            1 | 2 => {
                item.save_parent_id = Some(if item.id == 1 { DEMO_MOVIES } else { 0 });
                item.peers = Some(14 - item.id * 3);
                item.availability = Some(100);
                item.downloaded = Some(item.size * item.percent_done / 100);
                item.created_at = Some("2026-11-05T09:12:44".into());
            }
            5 => {
                item.downloaded = Some(item.size);
                item.uploaded = Some(item.size / 4);
                item.current_ratio = Some(0.25);
                item.created_at = Some("2026-11-04T21:03:10".into());
                item.finished_at = Some("2026-11-04T21:41:52".into());
            }
            6 => {
                item.save_parent_id = Some(DEMO_SHOWS);
                item.subscription_id = Some(6);
                item.peers = Some(9);
                item.availability = Some(100);
                item.downloaded = Some(item.size);
                item.uploaded = Some(item.size * 3 / 2);
                item.current_ratio = Some(1.5);
                item.seconds_seeding = Some(2 * 3600 + 14 * 60);
                item.is_private = Some(false);
                item.tracker_message = Some("Announce OK".into());
                item.created_at = Some("2026-11-03T18:20:05".into());
                item.finished_at = Some("2026-11-03T19:02:31".into());
            }
            7 => {
                item.peers = Some(0);
                item.availability = Some(0);
                item.tracker_message =
                    Some("Tracker returned: unregistered torrent (code 1)".into());
                item.created_at = Some("2026-11-05T10:01:12".into());
            }
            _ => {}
        }
        if item.source.is_none() && item.id != 4 {
            item.source = Some(format!("magnet:?xt=urn:btih:{:040x}", item.id));
        }
    }
}
