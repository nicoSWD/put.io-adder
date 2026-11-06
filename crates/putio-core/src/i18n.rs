//! Typed, compile-time-complete translations (English, German, Spanish).
//!
//! Every user-visible string is a [`Msg`] variant. [`Msg::text`] must provide all three languages
//! for every variant, so a missing translation is a compile error, not a blank label.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Locale {
    #[default]
    En,
    De,
    Es,
}

impl Locale {
    /// From a BCP-47-ish tag such as `de-DE`, `es_ES.UTF-8` or `en`. Unknown languages fall back to English.
    #[must_use]
    pub fn from_tag(tag: &str) -> Self {
        let language = tag
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match language.as_str() {
            "de" => Self::De,
            "es" => Self::Es,
            _ => Self::En,
        }
    }
}

macro_rules! tr {
    ($locale:expr, $en:expr, $de:expr, $es:expr) => {
        match $locale {
            Locale::En => $en,
            Locale::De => $de,
            Locale::Es => $es,
        }
    };
}

/// Every message the app can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    // Status line
    Ready,
    FetchingInfo,
    SigningIn,
    AuthRequired,
    AuthExpired,
    SignInFailed {
        reason: String,
    },
    Authenticated,
    AccountFailed,
    TransfersFailed,
    Offline,
    RateLimited {
        seconds: u64,
    },
    ServerError,
    Summary {
        pending: usize,
        finished: usize,
        failed: usize,
    },
    Adding {
        name: String,
    },
    Added,
    AddedClosing,
    AddFailed {
        reason: String,
    },
    Uploading {
        name: String,
    },
    Uploaded,
    UploadedClosing,
    UploadFailed {
        reason: String,
    },
    NotATorrent {
        reason: String,
    },
    NotAMagnet,
    Canceling,
    Canceled,
    CancelFailed,
    Streaming,
    StreamNoPlayer,
    StreamNotReady,
    StreamNothingToPlay,
    StreamFailed,
    // Sign-in panel
    LinkInstructions,
    LinkCodeLabel,
    LinkOpenButton,
    LinkRetryButton,
    LinkExpired,
    // Table
    ColName,
    SortSpeed,
    ColEta,
    ColSize,
    EtaDone,
    EtaFinishing,
    /// "3m 07s left"
    EtaLeft {
        eta: String,
    },
    // Transfer states shown on a row
    StatusQueued,
    StatusWaiting,
    StatusPreparing,
    StatusDownloading,
    StatusSeeding,
    StatusFailed,
    // Summary chips
    ChipActive,
    ChipDone,
    ChipFailed,
    // Transfers panel
    TransfersTitle,
    SortBy,
    SortAscending,
    SortDescending,
    SortStatus,
    SortProgress,
    EmptyTitle,
    EmptyHint,
    // Menus & context menu
    MenuCancelTransfer,
    MenuStopSeeding,
    MenuRemoveFromList,
    MenuStream,
    MenuCopyLink,
    DetailGroupTransfer,
    DetailGroupSharing,
    DetailGroupDates,
    MenuDownload,
    DownloadPreparing,
    DownloadStarted,
    DownloadNotReady,
    DownloadBusy,
    DownloadFailed,
    DownloadZipFailed,
    DownloadZipTimeout,
    DownloadBadUrl,
    DownloadStartedPartial {
        missing: usize,
    },
    DownloadOpenFailed,
    OpenFailed,
    SecretStoreUnavailable,
    SecretLoadFailed,
    SecretSaveFailed,
    SecretDeleteFailed,
    CopyCode,
    HandlerKeptOther,
    A11yClose,
    A11yBack,
    A11yTransferActions,
    MenuDetails,
    DetailsTitle,
    DetailStatus,
    DetailType,
    DetailDownloaded,
    DetailUploaded,
    DetailRatio,
    DetailSeedingFor,
    DetailPeers,
    DetailAvailability,
    DetailPrivate,
    DetailTrackerMessage,
    DetailError,
    DetailAdded,
    DetailFinished,
    DetailYes,
    DetailNo,
    KindTorrent,
    KindUrl,
    KindPlaylist,
    KindLiveStream,
    MenuCopyMagnet,
    MenuCopySource,
    MenuRetry,
    MenuClearFinished {
        count: usize,
    },
    Copied,
    Retrying,
    Retried,
    RetryFailed,
    Clearing,
    Cleared,
    ClearFailed,
    MenuOpenOnPutio,
    MenuPreferences,
    MenuCheckUpdates,
    MenuProjectPage,
    MenuSignOut,
    MenuQuit,
    TooltipToggleTransfers,
    // Preferences
    PrefsTitle,
    PrefsGeneral,
    PrefsQuitMagnet,
    PrefsQuitMagnetHint,
    PrefsQuitTorrent,
    PrefsQuitTorrentHint,
    PrefsTrashTorrent,
    PrefsTrashTorrentHint,
    PrefsCheckUpdatesHint,
    PrefsDownloads,
    PrefsSaveIn,
    PrefsSaveInHint,
    PrefsAccount,
    PrefsSignedInAs {
        name: String,
    },
    PrefsAbout,
    PrefsVersion {
        version: String,
    },
    PrefsUnofficial,
    PrefsChooseFolderTitle,
    PrefsUseFolder {
        name: String,
    },
    PrefsCancel,
    PrefsNoSubfolders,
    PrefsCheckUpdates,
    PrefsAppearance,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    PrefsDefaultFolderNone,
    PrefsChooseFolder,
    PrefsClearFolder,
    // Account header
    Bandwidth {
        used: String,
    },
    DiskLeft {
        left: String,
    },
    // Notifications
    NotificationTitle,
    NotificationBody {
        name: String,
    },
    // Updates
    UpdateAvailable {
        version: String,
    },
    UpdateOpenPage,
    HandlerPromptTitle,
    HandlerPromptBody,
    HandlerYes,
    HandlerNotNow,
    HandlerNever,
    HandlerRegistering,
    HandlerSet,
    HandlerMakeDefault,
    PrefsDefaultApp,
    PrefsDefaultAppIs,
    PrefsDefaultAppIsNot,
    PrefsDefaultAppChecking,
    HandlerFailed {
        reason: String,
    },
    UpdateInstallNow,
    UpdateRestartNow,
    UpdateUnpacking,
    UpdateRestarting,
    UpdateTryAgain,
    UpdateDownloading {
        percent: u8,
    },
    UpdateReady {
        version: String,
    },
    UpdateFailed {
        reason: String,
    },
    UpdateCannotInstall {
        reason: String,
    },
    UpToDate,
    UpdateCheckFailed,
}

impl Msg {
    #[must_use]
    #[allow(clippy::too_many_lines)] // one flat table of strings is the point
    pub fn text(&self, l: Locale) -> String {
        match self {
            Self::Ready => tr!(l, "Ready", "Bereit", "Listo").into(),
            Self::FetchingInfo => tr!(
                l,
                "Fetching info…",
                "Lade Informationen…",
                "Obteniendo información…"
            )
            .into(),
            Self::SigningIn => tr!(
                l,
                "Waiting for you to link this app…",
                "Warte auf die Verknüpfung dieser App…",
                "Esperando a que vincules esta app…"
            )
            .into(),
            Self::AuthRequired => tr!(
                l,
                "Sign in to put.io to get started",
                "Melde dich bei put.io an, um zu beginnen",
                "Inicia sesión en put.io para empezar"
            )
            .into(),
            Self::AuthExpired => tr!(
                l,
                "Your put.io session expired. Please sign in again.",
                "Deine put.io-Sitzung ist abgelaufen. Bitte melde dich erneut an.",
                "Tu sesión de put.io ha caducado. Inicia sesión de nuevo."
            )
            .into(),
            Self::SignInFailed { reason } => tr!(
                l,
                format!("Couldn't start sign-in: {reason}"),
                format!("Anmeldung konnte nicht gestartet werden: {reason}"),
                format!("No se pudo iniciar sesión: {reason}")
            ),
            Self::Authenticated => tr!(
                l,
                "Signed in and ready to go!",
                "Angemeldet und startklar!",
                "¡Sesión iniciada y listo!"
            )
            .into(),
            Self::AccountFailed => tr!(
                l,
                "Couldn't load account info",
                "Kontoinformationen konnten nicht geladen werden",
                "No se pudo obtener la información de la cuenta"
            )
            .into(),
            Self::TransfersFailed => tr!(
                l,
                "Couldn't load transfers",
                "Übertragungen konnten nicht geladen werden",
                "No se pudieron obtener las transferencias"
            )
            .into(),
            Self::Offline => tr!(
                l,
                "Can't reach put.io. Check your connection.",
                "put.io ist nicht erreichbar. Prüfe deine Verbindung.",
                "No se puede conectar con put.io. Revisa tu conexión."
            )
            .into(),
            Self::RateLimited { seconds } => tr!(
                l,
                format!("put.io asked us to slow down. Retrying in {seconds} s."),
                format!("put.io bittet um eine Pause. Neuer Versuch in {seconds} s."),
                format!("put.io pidió ir más despacio. Reintentando en {seconds} s.")
            ),
            Self::ServerError => tr!(
                l,
                "put.io is having problems. Trying again soon.",
                "put.io hat Probleme. Neuer Versuch in Kürze.",
                "put.io tiene problemas. Reintentando en breve."
            )
            .into(),
            Self::Summary {
                pending,
                finished,
                failed,
            } => {
                let (pending, finished, failed) = (*pending, *finished, *failed);
                let base = tr!(
                    l,
                    match pending {
                        1 => format!("1 pending transfer  ·  {finished} finished"),
                        n => format!("{n} pending transfers  ·  {finished} finished"),
                    },
                    match pending {
                        1 => format!("1 laufende Übertragung  ·  {finished} fertig"),
                        n => format!("{n} laufende Übertragungen  ·  {finished} fertig"),
                    },
                    {
                        let done = if finished == 1 { "terminada" } else { "terminadas" };
                        match pending {
                            1 => format!("1 transferencia pendiente  ·  {finished} {done}"),
                            n => format!("{n} transferencias pendientes  ·  {finished} {done}"),
                        }
                    }
                );
                if failed == 0 {
                    base
                } else {
                    let extra = tr!(
                        l,
                        format!("{failed} failed"),
                        format!("{failed} fehlgeschlagen"),
                        format!("{failed} con error")
                    );
                    format!("{base}  ·  {extra}")
                }
            }
            Self::Adding { name } => tr!(
                l,
                format!("Adding: {name}"),
                format!("Füge hinzu: {name}"),
                format!("Añadiendo: {name}")
            ),
            Self::Added => tr!(
                l,
                "Added to your transfers!",
                "Zu deinen Übertragungen hinzugefügt!",
                "¡Añadido a tus transferencias!"
            )
            .into(),
            Self::AddedClosing => tr!(
                l,
                "Added! Closing in 5 seconds…",
                "Hinzugefügt! Wird in 5 Sekunden geschlossen…",
                "¡Añadido! Cerrando en 5 segundos…"
            )
            .into(),
            Self::AddFailed { reason } => tr!(
                l,
                format!("Couldn't add the transfer: {reason}"),
                format!("Hinzufügen fehlgeschlagen: {reason}"),
                format!("No se pudo añadir: {reason}")
            ),
            Self::Uploading { name } => tr!(
                l,
                format!("Uploading torrent: {name}"),
                format!("Lade Torrent hoch: {name}"),
                format!("Subiendo torrent: {name}")
            ),
            Self::Uploaded => tr!(
                l,
                "Torrent uploaded!",
                "Torrent hochgeladen!",
                "¡Torrent subido!"
            )
            .into(),
            Self::UploadedClosing => tr!(
                l,
                "Torrent uploaded! Closing in 5 seconds…",
                "Torrent hochgeladen! Wird in 5 Sekunden geschlossen…",
                "¡Torrent subido! Cerrando en 5 segundos…"
            )
            .into(),
            Self::UploadFailed { reason } => tr!(
                l,
                format!("Couldn't upload the torrent: {reason}"),
                format!("Hochladen fehlgeschlagen: {reason}"),
                format!("No se pudo subir: {reason}")
            ),
            Self::NotATorrent { reason } => tr!(
                l,
                format!("Not a usable torrent file: {reason}"),
                format!("Keine brauchbare Torrent-Datei: {reason}"),
                format!("No es un archivo torrent válido: {reason}")
            ),
            Self::NotAMagnet => tr!(
                l,
                "That magnet link isn't valid",
                "Dieser Magnet-Link ist ungültig",
                "Ese enlace magnet no es válido"
            )
            .into(),
            Self::Canceling => tr!(
                l,
                "Canceling transfer…",
                "Übertragung wird abgebrochen…",
                "Cancelando transferencia…"
            )
            .into(),
            Self::Canceled => tr!(
                l,
                "Transfer canceled",
                "Übertragung abgebrochen",
                "Transferencia cancelada"
            )
            .into(),
            Self::CancelFailed => tr!(
                l,
                "Couldn't cancel the transfer",
                "Übertragung konnte nicht abgebrochen werden",
                "No se pudo cancelar la transferencia"
            )
            .into(),
            Self::Streaming => {
                tr!(l, "Opening in VLC…", "Öffne in VLC…", "Abriendo en VLC…").into()
            }
            Self::StreamNoPlayer => tr!(
                l,
                "VLC wasn't found. Install it to stream.",
                "VLC wurde nicht gefunden. Installiere es zum Streamen.",
                "No se encontró VLC. Instálalo para reproducir."
            )
            .into(),
            Self::StreamNotReady => tr!(
                l,
                "This transfer has no files yet",
                "Diese Übertragung hat noch keine Dateien",
                "Esta transferencia aún no tiene archivos"
            )
            .into(),
            Self::StreamNothingToPlay => tr!(
                l,
                "No video files found to play",
                "Keine abspielbaren Videodateien gefunden",
                "No hay archivos de vídeo para reproducir"
            )
            .into(),
            Self::StreamFailed => tr!(
                l,
                "Couldn't start streaming",
                "Streaming konnte nicht gestartet werden",
                "No se pudo iniciar la reproducción"
            )
            .into(),
            Self::LinkInstructions => tr!(
                l,
                "Go to put.io/link and enter this code:",
                "Öffne put.io/link und gib diesen Code ein:",
                "Ve a put.io/link e introduce este código:"
            )
            .into(),
            Self::LinkCodeLabel => tr!(l, "Your code", "Dein Code", "Tu código").into(),
            Self::LinkOpenButton => tr!(
                l,
                "Open put.io/link",
                "put.io/link öffnen",
                "Abrir put.io/link"
            )
            .into(),
            Self::LinkRetryButton | Self::UpdateTryAgain => {
                tr!(l, "Try again", "Erneut versuchen", "Reintentar").into()
            }
            Self::LinkExpired => tr!(
                l,
                "That code expired",
                "Dieser Code ist abgelaufen",
                "Ese código ha caducado"
            )
            .into(),
            Self::ColName => tr!(l, "Name", "Name", "Nombre").into(),
            Self::SortSpeed => tr!(l, "Speed", "Tempo", "Velocidad").into(),
            Self::ColEta => tr!(l, "ETA", "Restzeit", "Restante").into(),
            Self::ColSize => tr!(l, "Size", "Größe", "Tamaño").into(),
            Self::EtaDone => tr!(l, "Done", "Fertig", "Listo").into(),
            Self::EtaLeft { eta } => tr!(
                l,
                format!("{eta} left"),
                format!("noch {eta}"),
                format!("quedan {eta}")
            ),
            Self::StatusQueued => tr!(l, "In queue", "In der Warteschlange", "En cola").into(),
            Self::StatusWaiting => tr!(l, "Waiting…", "Wartet…", "Esperando…").into(),
            Self::StatusPreparing => tr!(l, "Preparing…", "Bereite vor…", "Preparando…").into(),
            Self::StatusDownloading => tr!(l, "Downloading", "Lädt herunter", "Descargando").into(),
            Self::StatusSeeding => tr!(
                l,
                "Done · seeding",
                "Fertig · seedet",
                "Listo · compartiendo"
            )
            .into(),
            Self::StatusFailed => tr!(l, "Failed", "Fehlgeschlagen", "Con error").into(),
            Self::TransfersTitle => tr!(l, "Transfers", "Übertragungen", "Transferencias").into(),
            Self::ChipActive => tr!(l, "active", "aktiv", "activas").into(),
            Self::ChipDone => tr!(l, "done", "fertig", "listas").into(),
            Self::ChipFailed => tr!(l, "failed", "fehlgeschlagen", "con error").into(),
            Self::SortBy => tr!(l, "Sort", "Sortieren", "Ordenar").into(),
            Self::SortAscending => tr!(l, "Ascending", "Aufsteigend", "Ascendente").into(),
            Self::SortDescending => tr!(l, "Descending", "Absteigend", "Descendente").into(),
            Self::SortStatus | Self::DetailStatus => tr!(l, "Status", "Status", "Estado").into(),
            Self::SortProgress => tr!(l, "Progress", "Fortschritt", "Progreso").into(),
            Self::EmptyTitle => tr!(
                l,
                "No transfers yet",
                "Noch keine Übertragungen",
                "Aún no hay transferencias"
            )
            .into(),
            Self::EmptyHint => tr!(
                l,
                "Click a magnet link or open a .torrent file, and it will show up here.",
                "Klicke auf einen Magnet-Link oder öffne eine .torrent-Datei. Die Übertragung erscheint dann hier.",
                "Haz clic en un enlace magnet o abre un archivo .torrent y aparecerá aquí."
            )
            .into(),
            Self::EtaFinishing => tr!(l, "Finishing", "Schließe ab", "Terminando").into(),
            Self::MenuCancelTransfer => tr!(
                l,
                "Cancel transfer",
                "Übertragung abbrechen",
                "Cancelar transferencia"
            )
            .into(),
            Self::MenuStopSeeding => {
                tr!(l, "Stop seeding", "Seeden beenden", "Dejar de compartir").into()
            }
            Self::MenuRemoveFromList => tr!(
                l,
                "Remove from list",
                "Aus der Liste entfernen",
                "Quitar de la lista"
            )
            .into(),
            Self::MenuStream => {
                tr!(l, "Stream in VLC", "In VLC streamen", "Reproducir en VLC").into()
            }
            Self::MenuDetails | Self::DetailsTitle => {
                tr!(l, "Details", "Details", "Detalles").into()
            }
            Self::DetailType => tr!(l, "Type", "Typ", "Tipo").into(),
            Self::DetailDownloaded => tr!(l, "Downloaded", "Heruntergeladen", "Descargado").into(),
            Self::DetailUploaded => tr!(l, "Uploaded", "Hochgeladen", "Subido").into(),
            Self::DetailRatio => tr!(l, "Share ratio", "Verhältnis", "Ratio").into(),
            Self::DetailSeedingFor => {
                tr!(l, "Seeding for", "Seed-Dauer", "Tiempo compartiendo").into()
            }
            Self::DetailPeers => tr!(l, "Peers", "Peers", "Pares").into(),
            Self::DetailAvailability => {
                tr!(l, "Availability", "Verfügbarkeit", "Disponibilidad").into()
            }
            Self::DetailPrivate => {
                tr!(l, "Private tracker", "Privater Tracker", "Tracker privado").into()
            }
            Self::DetailTrackerMessage => tr!(
                l,
                "Tracker message",
                "Tracker-Meldung",
                "Mensaje del tracker"
            )
            .into(),
            Self::DetailError => tr!(l, "Error", "Fehler", "Error").into(),
            Self::DetailAdded => tr!(l, "Added", "Hinzugefügt", "Añadido").into(),
            Self::DetailFinished => tr!(l, "Finished", "Abgeschlossen", "Finalizado").into(),
            Self::DetailYes => tr!(l, "Yes", "Ja", "Sí").into(),
            Self::DetailNo => tr!(l, "No", "Nein", "No").into(),
            Self::KindTorrent => tr!(l, "Torrent", "Torrent", "Torrent").into(),
            Self::KindUrl => tr!(l, "Link", "Link", "Enlace").into(),
            Self::KindPlaylist => tr!(l, "Playlist", "Playlist", "Lista de reproducción").into(),
            Self::KindLiveStream => {
                tr!(l, "Live stream", "Livestream", "Transmisión en vivo").into()
            }
            Self::MenuDownload => tr!(l, "Download", "Herunterladen", "Descargar").into(),
            Self::DownloadPreparing => tr!(
                l,
                "Preparing download…",
                "Bereite Download vor…",
                "Preparando descarga…"
            )
            .into(),
            Self::DownloadStarted => tr!(
                l,
                "Download started in your browser",
                "Download im Browser gestartet",
                "Descarga iniciada en el navegador"
            )
            .into(),
            Self::DownloadNotReady => tr!(
                l,
                "Nothing to download yet",
                "Noch nichts zum Herunterladen",
                "Aún no hay nada que descargar"
            )
            .into(),
            Self::DownloadBusy => tr!(
                l,
                "A folder is already being prepared",
                "Ein Ordner wird bereits vorbereitet",
                "Ya se está preparando una carpeta"
            )
            .into(),
            Self::DownloadFailed => tr!(
                l,
                "Couldn't start the download",
                "Download konnte nicht gestartet werden",
                "No se pudo iniciar la descarga"
            )
            .into(),
            Self::DownloadZipFailed => tr!(
                l,
                "put.io couldn't build the ZIP",
                "put.io konnte das ZIP nicht erstellen",
                "put.io no pudo crear el ZIP"
            )
            .into(),
            Self::DownloadZipTimeout => tr!(
                l,
                "Building the ZIP takes too long",
                "Das Erstellen des ZIP dauert zu lange",
                "Crear el ZIP tarda demasiado"
            )
            .into(),
            Self::DownloadBadUrl => tr!(
                l,
                "put.io returned an unexpected download address",
                "put.io lieferte eine unerwartete Download-Adresse",
                "put.io devolvió una dirección de descarga inesperada"
            )
            .into(),
            Self::DownloadStartedPartial { missing } => tr!(
                l,
                format!("Download started · put.io left out {missing} file(s)"),
                format!("Download gestartet · put.io hat {missing} Datei(en) ausgelassen"),
                format!("Descarga iniciada · put.io omitió {missing} archivo(s)")
            ),
            Self::DetailGroupTransfer => tr!(l, "Transfer", "Übertragung", "Transferencia").into(),
            Self::DetailGroupSharing => tr!(l, "Sharing", "Teilen", "Compartir").into(),
            Self::DetailGroupDates => tr!(l, "Dates", "Zeitpunkte", "Fechas").into(),
            Self::MenuCopyLink => tr!(
                l,
                "Copy put.io link",
                "put.io-Link kopieren",
                "Copiar enlace de put.io"
            )
            .into(),
            Self::MenuCopyMagnet => tr!(
                l,
                "Copy magnet link",
                "Magnet-Link kopieren",
                "Copiar enlace magnet"
            )
            .into(),
            Self::MenuCopySource => tr!(
                l,
                "Copy source link",
                "Quell-Link kopieren",
                "Copiar enlace de origen"
            )
            .into(),
            Self::MenuRetry => tr!(l, "Retry", "Erneut versuchen", "Reintentar").into(),
            Self::MenuClearFinished { count } => tr!(
                l,
                format!("Clear finished ({count})"),
                format!("Fertige entfernen ({count})"),
                format!("Quitar terminadas ({count})")
            ),
            Self::Copied => tr!(
                l,
                "Copied to the clipboard",
                "In die Zwischenablage kopiert",
                "Copiado al portapapeles"
            )
            .into(),
            Self::Retrying => tr!(l, "Retrying…", "Versuche erneut…", "Reintentando…").into(),
            Self::Retried => tr!(
                l,
                "Transfer restarted",
                "Übertragung neu gestartet",
                "Transferencia reiniciada"
            )
            .into(),
            Self::RetryFailed => tr!(
                l,
                "Couldn't retry the transfer",
                "Erneuter Versuch fehlgeschlagen",
                "No se pudo reintentar"
            )
            .into(),
            Self::Clearing => tr!(
                l,
                "Clearing finished transfers…",
                "Entferne fertige Übertragungen…",
                "Quitando transferencias terminadas…"
            )
            .into(),
            Self::Cleared => tr!(
                l,
                "Finished transfers cleared",
                "Fertige Übertragungen entfernt",
                "Transferencias terminadas quitadas"
            )
            .into(),
            Self::ClearFailed => tr!(
                l,
                "Couldn't clear the list",
                "Liste konnte nicht geleert werden",
                "No se pudo vaciar la lista"
            )
            .into(),
            Self::MenuOpenOnPutio => {
                tr!(l, "Open on put.io", "Auf put.io öffnen", "Abrir en put.io").into()
            }
            Self::MenuPreferences => tr!(l, "Preferences…", "Einstellungen…", "Ajustes…").into(),
            Self::MenuCheckUpdates => tr!(
                l,
                "Check for updates…",
                "Nach Updates suchen…",
                "Buscar actualizaciones…"
            )
            .into(),
            Self::MenuProjectPage => tr!(
                l,
                "Project page on GitHub",
                "Projektseite auf GitHub",
                "Página del proyecto en GitHub"
            )
            .into(),
            Self::MenuQuit => tr!(l, "Quit", "Beenden", "Salir").into(),
            Self::MenuSignOut => tr!(l, "Sign out", "Abmelden", "Cerrar sesión").into(),
            Self::TooltipToggleTransfers => tr!(
                l,
                "Show or hide transfers",
                "Übertragungen ein- oder ausblenden",
                "Mostrar u ocultar transferencias"
            )
            .into(),
            Self::PrefsGeneral => tr!(l, "General", "Allgemein", "General").into(),
            Self::PrefsQuitMagnet => tr!(
                l,
                "Quit after adding a magnet link",
                "Nach dem Hinzufügen eines Magnet-Links beenden",
                "Cerrar la app tras añadir un enlace magnet"
            )
            .into(),
            Self::PrefsQuitMagnetHint => tr!(
                l,
                "Closes the app 5 seconds after the link was sent to put.io.",
                "Schließt die App 5 Sekunden, nachdem der Link an put.io gesendet wurde.",
                "Cierra la app 5 segundos después de enviar el enlace a put.io."
            )
            .into(),
            Self::PrefsQuitTorrent => tr!(
                l,
                "Quit after uploading a .torrent file",
                "Nach dem Hochladen einer .torrent-Datei beenden",
                "Cerrar la app tras subir un archivo .torrent"
            )
            .into(),
            Self::PrefsQuitTorrentHint => tr!(
                l,
                "Closes the app 5 seconds after the upload finished.",
                "Schließt die App 5 Sekunden nach dem Hochladen.",
                "Cierra la app 5 segundos después de terminar la subida."
            )
            .into(),
            Self::PrefsTrashTorrent => tr!(
                l,
                "Move .torrent files to the Trash after uploading",
                "Hochgeladene .torrent-Dateien in den Papierkorb",
                "Mover los .torrent a la papelera tras subirlos"
            )
            .into(),
            Self::PrefsTrashTorrentHint => tr!(
                l,
                "Only once put.io has taken the file.",
                "Erst, nachdem put.io die Datei angenommen hat.",
                "Solo cuando put.io haya aceptado el archivo."
            )
            .into(),
            Self::PrefsCheckUpdatesHint => tr!(
                l,
                "Looks for new releases on GitHub from time to time. Installs only when you say so.",
                "Sucht von Zeit zu Zeit auf GitHub nach neuen Versionen. Installiert nur auf Wunsch.",
                "Busca versiones nuevas en GitHub de vez en cuando. Instala solo si lo pides."
            )
            .into(),
            Self::PrefsDownloads => tr!(l, "Downloads", "Downloads", "Descargas").into(),
            Self::PrefsSaveIn => tr!(
                l,
                "Save new transfers in",
                "Neue Übertragungen speichern in",
                "Guardar nuevas transferencias en"
            )
            .into(),
            Self::PrefsSaveInHint => tr!(
                l,
                "A folder on put.io. Without one, put.io's own default is used.",
                "Ein Ordner auf put.io. Ohne Auswahl gilt der Standardordner von put.io.",
                "Una carpeta en put.io. Sin elegir una, se usa la predeterminada de put.io."
            )
            .into(),
            Self::PrefsAccount => tr!(l, "Account", "Konto", "Cuenta").into(),
            Self::PrefsSignedInAs { name } => tr!(
                l,
                format!("Signed in as {name}"),
                format!("Angemeldet als {name}"),
                format!("Sesión iniciada como {name}")
            ),
            Self::PrefsAbout => tr!(l, "About", "Info", "Acerca de").into(),
            Self::PrefsVersion { version } => format!("put.io adder {version}"),
            Self::PrefsUnofficial => tr!(
                l,
                "Unofficial app. Not affiliated with put.io.",
                "Inoffizielle App. Nicht mit put.io verbunden.",
                "App no oficial. Sin afiliación con put.io."
            )
            .into(),
            Self::PrefsChooseFolderTitle => tr!(
                l,
                "Choose a folder",
                "Ordner auswählen",
                "Elige una carpeta"
            )
            .into(),
            Self::PrefsUseFolder { name } => tr!(
                l,
                format!("Use “{name}”"),
                format!("„{name}“ verwenden"),
                format!("Usar «{name}»")
            ),
            Self::PrefsCancel => tr!(l, "Cancel", "Abbrechen", "Cancelar").into(),
            Self::PrefsNoSubfolders => {
                tr!(l, "No subfolders", "Keine Unterordner", "Sin subcarpetas").into()
            }
            Self::PrefsTitle => tr!(l, "Preferences", "Einstellungen", "Ajustes").into(),
            Self::PrefsCheckUpdates => tr!(
                l,
                "Check for updates",
                "Nach Updates suchen",
                "Buscar actualizaciones"
            )
            .into(),
            Self::PrefsAppearance => tr!(l, "Appearance", "Darstellung", "Apariencia").into(),
            Self::ThemeSystem => tr!(l, "System", "System", "Sistema").into(),
            Self::ThemeLight => tr!(l, "Light", "Hell", "Claro").into(),
            Self::ThemeDark => tr!(l, "Dark", "Dunkel", "Oscuro").into(),
            Self::PrefsDefaultFolderNone => tr!(
                l,
                "Your put.io default folder",
                "Dein put.io-Standardordner",
                "Tu carpeta predeterminada de put.io"
            )
            .into(),
            Self::PrefsChooseFolder => tr!(l, "Choose…", "Auswählen…", "Elegir…").into(),
            Self::PrefsClearFolder => tr!(
                l,
                "Use default",
                "Standard verwenden",
                "Usar la predeterminada"
            )
            .into(),
            Self::Bandwidth { used } => tr!(
                l,
                format!("{used} this month"),
                format!("{used} in diesem Monat"),
                format!("{used} este mes")
            ),
            Self::DiskLeft { left } => tr!(
                l,
                format!("{left} left"),
                format!("{left} frei"),
                format!("{left} libres")
            ),
            Self::NotificationTitle => tr!(
                l,
                "Download finished",
                "Download abgeschlossen",
                "Descarga terminada"
            )
            .into(),
            Self::NotificationBody { name } => tr!(
                l,
                format!("“{name}” finished downloading"),
                format!("„{name}“ wurde heruntergeladen"),
                format!("«{name}» terminó de descargarse")
            ),
            Self::UpdateAvailable { version } => tr!(
                l,
                format!("Version {version} is available"),
                format!("Version {version} ist verfügbar"),
                format!("La versión {version} está disponible")
            ),
            Self::UpdateInstallNow => {
                tr!(l, "Update now", "Jetzt aktualisieren", "Actualizar ahora").into()
            }
            Self::UpdateRestartNow => tr!(
                l,
                "Restart to finish",
                "Zum Abschluss neu starten",
                "Reiniciar para terminar"
            )
            .into(),
            Self::UpdateUnpacking => tr!(
                l,
                "Preparing the update…",
                "Bereite das Update vor…",
                "Preparando la actualización…"
            )
            .into(),
            Self::UpdateRestarting => tr!(l, "Restarting…", "Starte neu…", "Reiniciando…").into(),
            Self::UpdateDownloading { percent } => tr!(
                l,
                format!("Updating… {percent}%"),
                format!("Aktualisiere… {percent} %"),
                format!("Actualizando… {percent} %")
            ),
            Self::UpdateReady { version } => tr!(
                l,
                format!("Version {version} is ready"),
                format!("Version {version} ist bereit"),
                format!("La versión {version} está lista")
            ),
            Self::UpdateFailed { reason } => tr!(
                l,
                format!("Update failed: {reason}"),
                format!("Update fehlgeschlagen: {reason}"),
                format!("Falló la actualización: {reason}")
            ),
            Self::UpdateCannotInstall { reason } => reason.clone(),
            Self::HandlerPromptTitle => tr!(
                l,
                "Open magnet links with put.io adder?",
                "Magnet-Links mit put.io adder öffnen?",
                "¿Abrir enlaces magnet con put.io adder?"
            )
            .into(),
            Self::HandlerPromptBody => tr!(
                l,
                "Make put.io adder your default app for magnet links and .torrent files, so a click sends them straight to your put.io queue.",
                "put.io adder zur Standard-App für Magnet-Links und .torrent-Dateien machen, damit ein Klick sie direkt in deine put.io-Warteschlange schickt.",
                "Haz de put.io adder tu app predeterminada para enlaces magnet y archivos .torrent, y un clic los enviará directamente a tu cola de put.io."
            )
            .into(),
            Self::HandlerYes => tr!(
                l,
                "Yes, make it the default",
                "Ja, als Standard festlegen",
                "Sí, usarla como predeterminada"
            )
            .into(),
            Self::HandlerNotNow => tr!(
                l,
                "Not now",
                "Jetzt nicht",
                "Ahora no"
            )
            .into(),
            Self::HandlerNever => tr!(
                l,
                "Don't ask again",
                "Nicht mehr fragen",
                "No volver a preguntar"
            )
            .into(),
            Self::HandlerRegistering => tr!(
                l,
                "Setting the default app…",
                "Lege die Standard-App fest…",
                "Configurando la app predeterminada…"
            )
            .into(),
            Self::HandlerSet => tr!(
                l,
                "put.io adder now opens magnet links and .torrent files",
                "put.io adder öffnet jetzt Magnet-Links und .torrent-Dateien",
                "put.io adder ahora abre enlaces magnet y archivos .torrent"
            )
            .into(),
            Self::HandlerMakeDefault => tr!(
                l,
                "Make default",
                "Als Standard festlegen",
                "Establecer como predeterminada"
            )
            .into(),
            Self::PrefsDefaultApp => tr!(
                l,
                "Default app",
                "Standard-App",
                "App predeterminada"
            )
            .into(),
            Self::PrefsDefaultAppIs => tr!(
                l,
                "put.io adder opens magnet links and .torrent files.",
                "put.io adder öffnet Magnet-Links und .torrent-Dateien.",
                "put.io adder abre enlaces magnet y archivos .torrent."
            )
            .into(),
            Self::PrefsDefaultAppIsNot => tr!(
                l,
                "Another app opens magnet links and .torrent files.",
                "Eine andere App öffnet Magnet-Links und .torrent-Dateien.",
                "Otra app abre enlaces magnet y archivos .torrent."
            )
            .into(),
            Self::PrefsDefaultAppChecking => tr!(
                l,
                "Checking…",
                "Prüfe…",
                "Comprobando…"
            )
            .into(),
            Self::HandlerFailed { reason } => tr!(
                l,
                format!("Couldn't set the default app: {reason}"),
                format!("Standard-App konnte nicht festgelegt werden: {reason}"),
                format!("No se pudo configurar la app predeterminada: {reason}")
            ),
            Self::UpdateOpenPage => tr!(l, "View release", "Release ansehen", "Ver versión").into(),
            Self::UpToDate => tr!(
                l,
                "You're up to date",
                "Du bist auf dem neuesten Stand",
                "Estás al día"
            )
            .into(),
            Self::DownloadOpenFailed => tr!(
                l,
                "Couldn't open your browser for the download",
                "Browser konnte für den Download nicht geöffnet werden",
                "No se pudo abrir el navegador para la descarga"
            )
            .into(),
            Self::OpenFailed => tr!(
                l,
                "Couldn't open the browser",
                "Browser konnte nicht geöffnet werden",
                "No se pudo abrir el navegador"
            )
            .into(),
            Self::SecretStoreUnavailable => tr!(
                l,
                "No secure storage here. You'll need to sign in again next time.",
                "Kein sicherer Speicher verfügbar. Du musst dich beim nächsten Mal erneut anmelden.",
                "No hay almacenamiento seguro. Tendrás que iniciar sesión de nuevo la próxima vez."
            )
            .into(),
            Self::SecretLoadFailed => tr!(
                l,
                "Couldn't read your saved sign-in",
                "Gespeicherte Anmeldung konnte nicht gelesen werden",
                "No se pudo leer tu inicio de sesión guardado"
            )
            .into(),
            Self::SecretSaveFailed => tr!(
                l,
                "Couldn't save your sign-in. You'll need to sign in again next time.",
                "Anmeldung konnte nicht gespeichert werden. Du musst dich beim nächsten Mal erneut anmelden.",
                "No se pudo guardar tu inicio de sesión. Tendrás que iniciar sesión de nuevo la próxima vez."
            )
            .into(),
            Self::SecretDeleteFailed => tr!(
                l,
                "Couldn't remove your saved sign-in from this computer",
                "Gespeicherte Anmeldung konnte nicht von diesem Computer entfernt werden",
                "No se pudo quitar tu inicio de sesión guardado de este equipo"
            )
            .into(),
            Self::CopyCode => tr!(l, "Copy code", "Code kopieren", "Copiar código").into(),
            Self::HandlerKeptOther => tr!(
                l,
                "the system kept another app for some of them",
                "Das System hat für einige davon eine andere App beibehalten",
                "el sistema mantuvo otra app para algunas de ellas"
            )
            .into(),
            Self::A11yClose => tr!(l, "Close", "Schließen", "Cerrar").into(),
            Self::A11yBack => tr!(l, "Back", "Zurück", "Atrás").into(),
            Self::A11yTransferActions => {
                tr!(l, "Actions", "Aktionen", "Acciones").into()
            }
            Self::UpdateCheckFailed => tr!(
                l,
                "Couldn't check for updates",
                "Updates konnten nicht geprüft werden",
                "No se pudo comprobar si hay actualizaciones"
            )
            .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags() {
        assert_eq!(Locale::from_tag("de-DE"), Locale::De);
        assert_eq!(Locale::from_tag("es_ES.UTF-8"), Locale::Es);
        assert_eq!(Locale::from_tag("ES"), Locale::Es);
        assert_eq!(Locale::from_tag("en-US"), Locale::En);
        assert_eq!(Locale::from_tag("fr"), Locale::En);
        assert_eq!(Locale::from_tag(""), Locale::En);
    }

    #[test]
    fn summary_pluralises_and_mentions_failures() {
        let one = Msg::Summary {
            pending: 1,
            finished: 4,
            failed: 0,
        };
        assert_eq!(one.text(Locale::En), "1 pending transfer  ·  4 finished");
        let many = Msg::Summary {
            pending: 3,
            finished: 0,
            failed: 2,
        };
        assert_eq!(
            many.text(Locale::En),
            "3 pending transfers  ·  0 finished  ·  2 failed"
        );
        assert_eq!(
            Msg::Summary {
                pending: 0,
                finished: 2,
                failed: 0
            }
            .text(Locale::De),
            "0 laufende Übertragungen  ·  2 fertig"
        );
    }

    #[test]
    fn the_spanish_summary_agrees_in_number() {
        let summary = |pending, finished| Msg::Summary {
            pending,
            finished,
            failed: 0,
        };
        assert_eq!(
            summary(1, 1).text(Locale::Es),
            "1 transferencia pendiente  ·  1 terminada"
        );
        assert_eq!(
            summary(2, 3).text(Locale::Es),
            "2 transferencias pendientes  ·  3 terminadas"
        );
    }

    #[test]
    fn arguments_are_interpolated_in_every_language() {
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let msg = Msg::NotificationBody {
                name: "Sintel".into(),
            };
            assert!(msg.text(locale).contains("Sintel"));
            assert!(Msg::RateLimited { seconds: 42 }.text(locale).contains("42"));
            assert!(Msg::Adding { name: "X".into() }.text(locale).contains('X'));
        }
    }

    #[test]
    fn no_translation_is_blank() {
        let samples = [
            Msg::Ready,
            Msg::AuthRequired,
            Msg::ColName,
            Msg::PrefsTitle,
            Msg::PrefsTrashTorrent,
            Msg::PrefsTrashTorrentHint,
            Msg::UpToDate,
            Msg::LinkInstructions,
            Msg::NotificationTitle,
            Msg::MenuSignOut,
            Msg::StreamNoPlayer,
            Msg::DownloadOpenFailed,
            Msg::OpenFailed,
            Msg::SecretStoreUnavailable,
            Msg::SecretLoadFailed,
            Msg::SecretSaveFailed,
            Msg::SecretDeleteFailed,
            Msg::CopyCode,
            Msg::HandlerKeptOther,
            Msg::A11yClose,
            Msg::A11yBack,
            Msg::A11yTransferActions,
        ];
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for msg in &samples {
                assert!(!msg.text(locale).trim().is_empty(), "{msg:?} / {locale:?}");
            }
        }
    }
}
