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
    vec![
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
