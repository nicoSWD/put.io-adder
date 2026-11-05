//! Sample data for looking at the UI without an account. Debug builds only
//! (`PUTIO_ADDER_DEMO=1 cargo run -p putio-adder`): this module does not exist in release builds.

use putio_api::{Account, Disk, Transfer, TransferKind, TransferStatus};

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
fn add_details(list: &mut [Transfer]) {
    for item in list {
        match item.id {
            1 | 2 => {
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
