//! Russian and English strings. Each string is an enum variant, so a missing translation is a
//! compile error and a lookup is a `match`, cheap enough to call all over the view.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lang {
    Ru,
    #[default]
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        match code.to_ascii_lowercase().get(..2)? {
            "ru" | "be" | "uk" | "kk" => Some(Lang::Ru),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// The UI language the OS is set to, falling back to English.
    pub fn system() -> Lang {
        for var in ["GLINT_LANG", "LC_ALL", "LANG"] {
            if let Some(l) = std::env::var(var).ok().and_then(|v| Lang::from_code(&v)) {
                return l;
            }
        }
        #[cfg(windows)]
        if let Some(l) = windows_locale() {
            return l;
        }
        #[cfg(target_os = "macos")]
        if let Some(l) = mac_locale() {
            return l;
        }
        Lang::En
    }

    pub fn tracks(self, n: usize) -> String {
        match self {
            Lang::En => format!("{n} {}", if n == 1 { "track" } else { "tracks" }),
            Lang::Ru => format!("{n} {}", plural_ru(n, "трек", "трека", "треков")),
        }
    }

    /// "1 h 05 min" / "1 ч 05 мин" for a total play time.
    pub fn duration(self, secs: u64) -> String {
        let (h, m) = (secs / 3600, (secs / 60) % 60);
        match (self, h) {
            (Lang::En, 0) => format!("{m} min"),
            (Lang::En, _) => format!("{h} h {m:02} min"),
            (Lang::Ru, 0) => format!("{m} мин"),
            (Lang::Ru, _) => format!("{h} ч {m:02} мин"),
        }
    }

    /// Formats an approximate byte count as `~12 MB` / `~1.4 GB` (`~12 МБ` / `~1.4 ГБ`).
    pub fn approx_size(self, bytes: u64) -> String {
        let mb = bytes as f64 / (1024.0 * 1024.0);
        if mb >= 1024.0 {
            let gb = mb / 1024.0;
            match self {
                Lang::En => format!("~{gb:.1} GB"),
                Lang::Ru => format!("~{gb:.1} ГБ"),
            }
        } else {
            let mb = mb.round().max(1.0) as u64;
            match self {
                Lang::En => format!("~{mb} MB"),
                Lang::Ru => format!("~{mb} МБ"),
            }
        }
    }

    /// The month of a date, as it is written after the day ("13 November 2026" / "13 ноября
    /// 2026").
    pub fn month(self, n: u32) -> &'static str {
        const EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
        const RU: [&str; 12] = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];
        let i = (n.max(1) as usize).min(12) - 1;
        match self {
            Lang::En => EN[i],
            Lang::Ru => RU[i],
        }
    }
}

/// A release date, as a day to read: "2026-11-13T00:00:00+03:00" becomes "13 November 2026".
/// A date in another shape (a year on its own, say) is left as it is.
/// A count with its thousands set apart by (no-break) spaces: "1 234 567".
pub fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{a0}');
        }
        out.push(c);
    }
    out
}

pub fn release_date(date: &str, lang: Lang) -> String {
    let date = date.trim();
    let Some(head) = date.get(..10) else { return date.to_owned() };
    let mut parts = head.split('-');
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else { return date.to_owned() };
    let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>()) else { return date.to_owned() };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return date.to_owned();
    }
    format!("{d} {} {y}", lang.month(m))
}

fn plural_ru(n: usize, one: &'static str, few: &'static str, many: &'static str) -> &'static str {
    let (m10, m100) = (n % 10, n % 100);
    if m10 == 1 && m100 != 11 {
        one
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        few
    } else {
        many
    }
}

/// The first language in macOS's list of preferred ones (an app started from Finder has no
/// `LANG`): `defaults` prints the list as `("ru-RU", en)`, one entry a line.
#[cfg(target_os = "macos")]
fn mac_locale() -> Option<Lang> {
    let out = std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.split([',', '(', ')', '\n']).map(|t| t.trim().trim_matches('"')).find(|t| !t.is_empty())?;
    Lang::from_code(first)
}

#[cfg(windows)]
fn windows_locale() -> Option<Lang> {
    // `GetUserDefaultLocaleName` lives in kernel32; ask for it by hand to avoid a feature flag.
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    // SAFETY: `buf` is a writable 85-u16 buffer (LOCALE_NAME_MAX_LENGTH) matching `buf.len()`.
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return None;
    }
    Lang::from_code(&String::from_utf16_lossy(&buf[..(n as usize - 1).min(buf.len())]))
}

macro_rules! strings {
    ($($id:ident => $en:literal, $ru:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum S { $($id),* }
        impl S {
            pub fn get(self, lang: Lang) -> &'static str {
                match (self, lang) { $( (S::$id, Lang::En) => $en, (S::$id, Lang::Ru) => $ru, )* }
            }
        }
    };
}

strings! {
    // navigation
    Home => "Home", "Главная";
    Explore => "Explore", "Обзор";
    Forward => "Forward", "Вперёд";
    ScrollPrev => "Previous", "Предыдущие";
    ScrollNext => "Next", "Следующие";
    Search => "Search", "Поиск";
    Liked => "Liked", "Мне нравится";
    Settings => "Settings", "Настройки";
    FindSetting => "Search", "Поиск";
    Account => "Account", "Аккаунт";
    AboutApp => "About", "О приложении";
    Queue => "Queue", "Очередь";
    UpNext => "Up next", "Далее";
    Lyrics => "Lyrics", "Текст";
    Back => "Back", "Назад";
    Close => "Close", "Закрыть";
    Cancel => "Cancel", "Отмена";
    Save => "Save", "Сохранить";
    Create => "Create", "Создать";
    NewPlaylist => "New playlist", "Новый плейлист";
    PlaylistName => "Playlist name", "Название плейлиста";
    PlaylistCreated => "Playlist created", "Плейлист создан";
    AddToPlaylist => "Add to playlist", "Добавить в плейлист";
    AddedToPlaylist => "Added to", "Добавлено в";
    RemoveFromPlaylist => "Remove from playlist", "Убрать из плейлиста";
    RemovedFromPlaylist => "Removed from the playlist", "Убрано из плейлиста";
    Rename => "Rename", "Переименовать";
    RenamePlaylist => "Rename playlist", "Переименовать плейлист";
    PlaylistRenamed => "Playlist renamed", "Плейлист переименован";
    DeletePlaylist => "Delete playlist", "Удалить плейлист";
    DeletePlaylistNote => "It will be removed from your collection on all devices. This cannot be undone.", "Плейлист исчезнет из коллекции на всех устройствах. Вернуть его не получится.";
    PlaylistDeleted => "Playlist deleted", "Плейлист удалён";
    NoOwnPlaylists => "No playlists of your own yet", "Своих плейлистов пока нет";
    // home
    Play => "Play", "Слушать";
    Pause => "Pause", "Пауза";
    Shuffle => "Shuffle", "Вперемешку";
    Repeat => "Repeat", "Повтор";
    Next => "Next", "Далее";
    Previous => "Previous", "Назад";
    Like => "Like", "Нравится";
    Unlike => "Remove like", "Убрать лайк";
    Dislike => "Dislike", "Не нравится";
    Disliked => "We'll play less like this", "Таких треков станет меньше";
    Profile => "Profile", "Профиль";
    ShelfPersonal => "Made for you", "Для вас";
    ShelfEditorial => "Fresh picks", "Подборки";
    ShelfNew => "New releases", "Новые релизы";
    ShelfChart => "Chart", "Чарт";
    Nothing => "Nothing here yet", "Здесь пока пусто";
    Retry => "Try again", "Повторить";
    // search
    SearchHint => "Songs, artists, albums, playlists…", "Треки, исполнители, альбомы, плейлисты…";
    Tracks => "Tracks", "Треки";
    Albums => "Albums", "Альбомы";
    Artists => "Artists", "Исполнители";
    Playlists => "Playlists", "Плейлисты";
    NoResults => "No results", "Ничего не найдено";
    ShowingFor => "Showing results for “{}”", "Результаты по запросу «{}»";
    TopResult => "Top result", "Лучший результат";
    ShowAll => "Show all", "Показать все";
    Browse => "Browse by mood", "Музыка под настроение";
    Track => "Track", "Трек";
    LikedTracks => "Liked tracks", "Мне нравится";
    MonthlyListeners => "Monthly listeners", "Слушателей в месяц";
    ListenersMonth => "Listeners this month", "Слушателей за месяц";
    ListenersMore => "more than in the 30 days before", "больше, чем за предыдущие 30 дней";
    ListenersFewer => "fewer than in the 30 days before", "меньше, чем за предыдущие 30 дней";
    ListenersBy => "", "На";
    SheetCredits => "Credits", "Участники";
    ArtistPhotos => "Photos", "Фотографии";
    Label => "Label", "Лейбл";
    Labels => "Labels", "Лейблы";
    LikesCount => "Likes", "Нравится";
    FromRelease => "From the release", "Из релиза";
    ListenersPerMonth => "listeners a month", "слушателей за месяц";
    RecentSearches => "Recent searches", "Недавние запросы";
    ListeningHistory => "Listening history", "История прослушивания";
    Download => "Download", "Скачать";
    DownloadAll => "Download all", "Скачать всё";
    DownloadConfirm => "Download all tracks?", "Скачать все треки?";
    DownloadQuality => "Download quality", "Качество скачивания";
    DeleteDownload => "Delete download", "Удалить скачанное";
    DeleteDownloadConfirm => "Delete downloaded track?", "Удалить скачанный трек?";
    DeleteDownloadNote => "The file will be deleted from this computer.", "Файл будет удалён с этого компьютера.";
    DeleteDownloadsConfirm => "Delete downloaded tracks?", "Удалить скачанные треки?";
    DeleteDownloadsNote => "The files will be deleted from this computer.", "Файлы будут удалены с этого компьютера.";
    DownloadDeleted => "Download removed", "Скачанное удалено";
    DownloadDeleteFailed => "Couldn't remove the download", "Не удалось удалить скачанное";
    DownloadInProgress => "Downloading", "Скачивается";
    DownloadDone => "Downloaded", "Скачано";
    DownloadFailed => "Download failed", "Не удалось скачать";
    DlWhyOffline => "No connection", "Нет соединения";
    DlWhyUnavailable => "This track can't be downloaded", "Этот трек нельзя скачать";
    DlWhyRegion => "Not available in your region", "Недоступен в вашем регионе";
    DlWhySignIn => "Sign in again to download", "Чтобы скачать, войдите заново";
    DlWhyStorage => "Couldn't save the file", "Не удалось сохранить файл";
    DlWhyService => "The service is busy, try later", "Сервис занят, попробуйте позже";
    ClearHistory => "Clear history", "Очистить историю";
    Forget => "Remove from history", "Удалить из истории";
    AddToCollection => "Add to collection", "Добавить в коллекцию";
    RemoveFromCollection => "Remove from collection", "Убрать из коллекции";
    AddedToCollection => "Added to your collection", "Добавлено в коллекцию";
    RemovedFromCollection => "Removed from your collection", "Убрано из коллекции";
    LinkCopied => "Link copied", "Ссылка скопирована";
    Details => "Details", "Подробнее";
    AboutArtist => "About the artist", "Об артисте";
    SupportArtist => "Support the artist", "Поддержать артиста";
    ArtistLinks => "Links", "Ссылки";
    WaveByTrack => "My Wave by track", "Моя волна по треку";
    WaveByArtist => "My Wave by artist", "Моя волна по исполнителю";
    WaveByRelease => "My Wave by release", "Моя волна по релизу";
    WaveByCollection => "My Wave by collection", "Моя волна по коллекции";
    WaveByPlaylist => "My Wave by playlist", "Моя волна по плейлисту";
    ByTrack => "By track", "По треку";
    ByArtist => "By artist", "По исполнителю";
    ByRelease => "By release", "По релизу";
    MoreWaves => "More waves", "Другие волны";
    StageLook => "Player look", "Вид плеера";
    StageReset => "Reset", "Сбросить";
    StageBack => "Background", "Фон";
    Listen => "Listen", "Слушать";
    Pin => "Pin", "Закрепить";
    Pinned => "Pinned", "Закреплено";
    Unpin => "Unpin", "Открепить";
    Unpinned => "Unpinned", "Откреплено";
    Open => "Open", "Открыть";
    DownloadManager => "Downloads", "Загрузки";
    OpenDownloads => "Open downloads folder", "Открыть папку загрузок";
    ClearDownloadHistory => "Clear download history", "Очистить историю загрузок";
    DownloadHistoryCleared => "Download history cleared", "История загрузок очищена";
    DownloadAllLiked => "Download Liked", "Скачать «Мне нравится»";
    LyricAlign => "Alignment", "Выравнивание";
    AlignLeft => "Left", "Слева";
    AlignCenter => "Centre", "По центру";
    AlignRight => "Right", "Справа";
    LyricText => "Lyrics", "Текст";
    LyricSize => "Size", "Размер";
    SizeSmall => "Small", "Мелкий";
    SizeMedium => "Medium", "Средний";
    SizeLarge => "Large", "Крупный";
    SearchCollection => "Search", "Поиск";
    Collection => "Collection", "Коллекция";
    Resume => "Resume", "Продолжить";
    CopyTitle => "Copy title", "Скопировать название";
    TitleCopied => "Title copied", "Название скопировано";
    CopyLink => "Copy link", "Скопировать ссылку";
    OpenInYandexMusic => "Open in Yandex Music", "Открыть в Яндекс Музыке";
    Explicit => "Explicit", "Ненормативная лексика";
    ExplicitHint => "Explicit: contains strong language", "Содержит ненормативную лексику";
    CleanLoaded => "Uncensored version", "Версия без цензуры";
    PlayAllNext => "Play next", "Играть следующими";
    // library
    PopularTracks => "Popular tracks", "Популярные треки";
    AllTracks => "All tracks", "Все треки";
    All => "All", "Все";
    SetGeneral => "General", "Основные";
    SetAppearance => "Appearance", "Оформление";
    SetGraphics => "Graphics", "Графика";
    SetSystem => "System", "Система";
    SetPlayer => "Player", "Плеер";
    SetLyrics => "Lyrics", "Текст песни";
    Year => "Year", "Год";
    Genre => "Genre", "Жанр";
    Genres => "Genres", "Жанры";
    Duration => "Duration", "Длительность";
    ReleaseDate => "Release date", "Дата релиза";
    Description => "Description", "Описание";
    NoLyricsShort => "No lyrics", "Нет текста";
    Discography => "Discography", "Дискография";
    SimilarArtists => "Similar artists", "Похожие исполнители";
    // wave
    WaveTitle => "My Wave", "Моя волна";
    WaveShort => "Wave", "Волна";
    HistoryShort => "History", "История";
    WaveOnYour => "On your wave", "Моя волна";
    WaveFavorite => "Favorite", "Любимое";
    WaveDiscover => "Discover", "Незнакомое";
    WavePopular => "Popular", "Популярное";
    WaveEnergetic => "Energetic", "Бодрое";
    WaveCalm => "Calm", "Спокойное";
    WaveFun => "Fun", "Весёлое";
    WaveSad => "Sad", "Грустное";
    WaveWork => "Focus", "Фон для работы";
    WaveNoWords => "Wordless", "Без слов";
    WaveRussian => "Russian", "Русский";
    WaveForeign => "Foreign", "Иностранный";
    // now playing
    NowPlaying => "Now playing", "Сейчас играет";
    NoLyrics => "This track has no lyrics", "У этого трека нет текста";
    Instrumental => "Instrumental", "Инструментал";
    PlayNext => "Play next", "Играть следующим";
    AddToQueue => "Add to queue", "Добавить в очередь";
    RemoveFromQueue => "Remove from queue", "Убрать из очереди";
    GoToAlbum => "Go to album", "Перейти к альбому";
    GoToArtist => "Go to artist", "Перейти к исполнителю";
    QueueEmpty => "The queue is empty", "Очередь пуста";
    QueueCleared => "Queue cleared", "Очередь очищена";
    // login
    Welcome => "Sign in to Glint", "Вход в Glint";
    LoginLead => "An unofficial Yandex Music client.", "Неофициальный клиент Яндекс Музыки.";
    LoginStep1 => "Sign in to your Yandex account in the browser", "Войдите в аккаунт Яндекса в браузере";
    LoginStep2 => "Paste the page address or token", "Вставьте адрес страницы или токен";
    OpenYandex => "Open Yandex sign-in", "Открыть страницу входа Яндекса";
    CopyLoginLink => "Copy the sign-in link", "Скопировать ссылку для входа";
    Paste => "Paste", "Вставить";
    TokenHint => "Paste the redirect URL or token", "Вставьте адрес или токен";
    ShowToken => "Show token", "Показать токен";
    HideToken => "Hide token", "Скрыть токен";
    SignIn => "Sign in", "Войти";
    SigningIn => "Signing in…", "Выполняется вход…";
    BadToken => "That doesn't look like a token. Copy the full address after signing in.", "Не удалось распознать токен. Скопируйте адрес целиком после входа.";
    TokenRejected => "Yandex rejected that token.", "Яндекс отклонил этот токен.";
    PrivacyNote => "The token is encrypted on this computer and sent only to Yandex servers.", "Токен шифруется на этом компьютере и передаётся только на серверы Яндекса.";
    PrivacyNoteFile => "The token is stored in a private file and sent only to Yandex servers.", "Токен хранится в файле с доступом только для вас и передаётся только на серверы Яндекса.";
    SignOut => "Sign out", "Выйти";
    // settings
    Theme => "Theme", "Тема";
    ThemeDark => "Dark", "Тёмная";
    ThemeLight => "Light", "Светлая";
    ThemeSystem => "System", "Системная";
    Accent => "Accent colour", "Цвет акцента";
    AccentAdaptive => "From cover", "В цвет обложки";
    Effects => "Background effects", "Эффекты фона";
    Shaders => "Shaders", "Шейдеры";
    Butterflies => "Butterflies on Home", "Бабочки на главной";
    CpuMode => "CPU mode", "Режим процессора";
    CpuModeNote => "Uses less memory, restarts the app", "Меньше памяти, перезапустит приложение";
    ReduceMotion => "Disable animations", "Отключить анимации";
    Language => "Language", "Язык";
    Quality => "Audio quality", "Качество звука";
    QualitySaver => "Saver", "Экономия";
    QualityBalanced => "Balanced", "Баланс";
    QualityBest => "Best", "Лучшее";
    QualityMax => "Max", "Макс.";
    QualityNow => "Playing", "Сейчас";
    QualityMaxForTrack => "Max for track", "Максимум для трека";
    Kbps => "kbps", "кбит/с";
    PlayerLayout => "Layout", "Макет";
    PlayerLayoutAuto => "Auto", "Авто";
    PlayerLayoutCompact => "Compact", "Компакт";
    PlayerLayoutComfortable => "Standard", "Стандарт";
    PlayerLayoutCover => "Cover", "Обложка";
    Equalizer => "Equaliser", "Эквалайзер";
    EqEnabled => "Enable equaliser", "Включить эквалайзер";
    EqReset => "Reset", "Сбросить";
    TrayIcon => "Tray icon", "Значок в трее";
    CloseToTray => "Close to tray", "Закрывать в трей";
    TrayShow => "Open Glint", "Открыть Glint";
    TrayQuit => "Quit", "Выйти";
    TrayPlay => "Play", "Воспроизвести";
    TrayPause => "Pause", "Пауза";
    TrayPrev => "Previous track", "Предыдущий трек";
    TrayNext => "Next track", "Следующий трек";
    TrayLike => "Add to Liked", "Добавить в «Мне нравится»";
    TrayUnlike => "Remove from Liked", "Убрать из «Мне нравится»";
    TrayShuffleOn => "Shuffle: on", "Вперемешку: вкл.";
    TrayShuffleOff => "Shuffle: off", "Вперемешку: выкл.";
    TrayRepeatOff => "Repeat: off", "Повтор: выкл.";
    TrayRepeatAll => "Repeat: all", "Повтор: весь список";
    TrayRepeatOne => "Repeat: this track", "Повтор: этот трек";
    TrayMute => "Mute", "Выключить звук";
    TrayUnmute => "Unmute", "Включить звук";
    TrayNothing => "Nothing is playing", "Ничего не играет";
    CacheImages => "Cover images", "Обложки";
    CacheAudio => "Downloaded audio", "Скачанное аудио";
    CacheMetadata => "Track metadata", "Метаданные треков";
    AboutText => "Unofficial Yandex Music client", "Неофициальный клиент Яндекс Музыки";
    // toasts & errors
    Offline => "Can't reach Yandex Music", "Нет связи с Яндекс Музыкой";
    NotAvailable => "This track isn't available in your region", "Трек недоступен в вашем регионе";
    PlaybackFailed => "Couldn't play this track", "Не удалось воспроизвести трек";
    NoAudioDevice => "No audio output device", "Нет устройства вывода звука";
    SignedOut => "Signed out", "Вы вышли";
    QueuedToast => "Added to the queue", "Добавлено в очередь";
    PlayNextToast => "Playing next", "Сыграет следующим";
    // misc
    Album => "Album", "Альбом";
    Single => "Single", "Сингл";
    Compilation => "Compilation", "Сборник";
    SortBy => "Sort", "Сортировка";
    SortOriginal => "In order", "По порядку";
    SortRecent => "Recently added", "Сначала недавние";
    SortTitle => "By title", "По названию";
    SortNameAsc => "A to Z", "По алфавиту";
    SortNameDesc => "Z to A", "От Я до А";
    SortArtist => "By artist", "По исполнителю";
    SortAlbum => "By album", "По альбому";
    SortMostTracks => "Most tracks", "Больше треков";
    SortLongest => "Longest first", "Сначала длинные";
    SortShortest => "Shortest first", "Сначала короткие";
    SortNewest => "Newest first", "Сначала новые";
    SortOldest => "Oldest first", "Сначала старые";
    ShowKinds => "Show", "Показывать";
    KindAll => "Everything", "Всё";
    KindSingles => "Singles and EPs", "Синглы и EP";
    KindCompilations => "Compilations", "Сборники";
    NoneOfKind => "Nothing of this kind", "Таких релизов нет";
    Playlist => "Playlist", "Плейлист";
    Artist => "Artist", "Исполнитель";
    Plus => "Plus", "Плюс";
    More => "More", "Ещё";
    Mute => "Mute", "Без звука";
    Volume => "Volume", "Громкость";
    Flat => "Flat", "Ровно";
    BassBoost => "Bass boost", "Больше баса";
    Vocal => "Vocal", "Вокал";
    Bright => "Bright", "Яркий";
    ClearSearch => "Clear", "Очистить";
    Updates => "Updates", "Обновления";
    AutoUpdate => "Check when Glint starts", "Проверять при запуске";
    CheckNow => "Check now", "Проверить";
    UpdateChecking => "Checking…", "Проверка…";
    UpToDate => "You have the latest version", "У вас последняя версия";
    UpdateAvailable => "Update available:", "Доступно обновление:";
    UpdateInstall => "Update and restart", "Обновить и перезапустить";
    UpdateReleasePage => "Release page", "Страница релиза";
    UpdateDownloading => "Downloading", "Загрузка";
    UpdateInstalling => "Installing…", "Установка…";
    UpdateMenuSub => "New version available", "Доступна новая версия";
    UpdateShort => "Update", "Обновление";
    UpdateFailed => "Could not update. Try again, or download the new version from GitHub.", "Не удалось обновить. Попробуйте ещё раз или скачайте новую версию с GitHub.";
    UpdateManual => "Download the new version from the release page.", "Скачайте новую версию на странице релиза.";
    UpdateToast => "A new version is available", "Доступна новая версия";
    SourceCode => "Source code", "Исходный код";
    EqPreset => "Preset", "Пресет";
    License => "GPL-3.0 licence", "Лицензия GPL-3.0";
    WordLyrics => "Word-by-word lyrics", "Текст по словам";
    FckCensor => "FckCensor", "FckCensor";
    PlayOnLaunch => "Play on launch", "Играть при запуске";
    FckCensorNote => "Uncensored original tracks", "Оригинальные версии без цензуры";
    FkCensorNoCredits => "This track comes from FckCensor. Yandex Music has no credits for it, so only its artist, album and length are shown.", "Этот трек из FckCensor. У Яндекс Музыки нет сведений о нём, поэтому здесь только исполнитель, альбом и длительность.";
    SetHistory => "History", "История";
    SearchHistory => "Search history", "История поиска";
    ClearShort => "Clear", "Очистить";
    ClearQueue => "Clear queue", "Очистить очередь";
    HistoryEmpty => "Empty", "Пусто";
    HistoryCleared => "History cleared", "История очищена";
    IdHidden => "Hover to show", "Наведите, чтобы показать";
    SetSound => "Sound", "Звук";
    SetPlayerScreen => "Full screen", "Экран плеера";
    LyricsFont => "Font", "Шрифт";
    Autostart => "Open at login", "Запуск с системой";
    AutostartHidden => "Start in tray", "Запускать в трее";
    Backdrop => "Your picture", "Своя картинка";
    BdPick => "Choose", "Выбрать";
    BdChange => "Replace", "Заменить";
    BdRemove => "Remove", "Убрать";
    BdPicture => "Your picture", "Своя картинка";
    LyricFace => "Font", "Шрифт";
    BdImages => "Images", "Изображения";
    BdFailed => "This picture can't be opened", "Не удалось открыть эту картинку";
    BdBlur => "Blur", "Размытие";
    BdDim => "Darkening", "Затемнение";
    BdEffect => "Effect", "Эффект";
    BdNone => "None", "Нет";
    BdDither => "Dither", "Дизеринг";
    BdGrain => "Grain", "Зерно";
    BdPixels => "Pixels", "Пиксели";
    BdHalftone => "Halftone", "Растр";
    BdLines => "Scanlines", "Строки";
    Storage => "Storage", "Хранилище";
    DeleteContent => "Clear", "Очистить";
    ContentDeleted => "Downloaded content deleted", "Скачанное удалено";
    StorageUsed => "Used on disk", "Занято на диске";
    Version => "Version", "Версия";
    UpdateIdleNote => "Check for updates", "Проверить обновления";
    SetKeys => "Shortcuts", "Клавиши";
    ScPlay => "Play or pause", "Пауза и воспроизведение";
    ScSeekBack => "Back 5 seconds", "Назад на 5 секунд";
    ScSeekForward => "Forward 5 seconds", "Вперёд на 5 секунд";
    ScPrev => "Previous track", "Предыдущий трек";
    ScNext => "Next track", "Следующий трек";
    ScVolumeUp => "Volume up", "Громче";
    ScVolumeDown => "Volume down", "Тише";
    ScPageBack => "Previous page", "Предыдущая страница";
    ScPageForward => "Next page", "Следующая страница";
    KeyNone => "None", "Нет";
    KeyPress => "Press a key", "Нажмите клавишу";
    KeyHint => "Click a shortcut to change it. Backspace clears it.", "Нажмите на сочетание, чтобы изменить. Backspace очищает.";
    KeyTaken => "That key is already used for", "Эта клавиша уже занята:";
    ScLike => "Like", "Нравится";
    ScMute => "Mute", "Без звука";
    ScNowPlaying => "Now playing screen", "Экран плеера";
    ScSearch => "Search", "Поиск";
    ScBack => "Back, or close a panel", "Назад или закрыть панель";
    Links => "Links", "Ссылки";
    ReportIssue => "Report a problem", "Сообщить о проблеме";
    Diagnostics => "Diagnostics", "Диагностика";
    DiagLogs => "Diagnostic logs", "Запись логов";
    DiagLogFiles => "Log files", "Файлы логов";
    OpenLogs => "Open folder", "Открыть папку";
    LogsCleared => "Logs cleared", "Логи очищены";
    LyricsBy => "Lyrics by lrc.red", "Текст: lrc.red";
    SearchTrack => "Search track", "Поиск трека";
    UploadTrack => "Upload track", "Загрузить трек";
    AudioFiles => "Audio files", "Аудиофайлы";
    TrackUploaded => "Track added to the playlist", "Трек добавлен в плейлист";
    TrackUploadFailed => "Couldn't load this audio file", "Не удалось загрузить аудиофайл";
    MiniPlayer => "Mini player", "Мини-плеер";
    MiniStyle => "Layout", "Вид";
    MiniCompact => "Bar", "Полоска";
    MiniStandard => "Card", "Карточка";
    MiniCover => "Cover", "Обложка";
    MiniAlwaysOnTop => "Always on top", "Поверх всех окон";
    MiniOpacity => "Opacity", "Прозрачность";
    MiniOnClose => "On window close", "При закрытии окна";
    MiniExpand => "Back to main window", "Вернуться в главное окно";
    QualitySaverDesc => "AAC · 64 kbps", "AAC · 64 кбит/с";
    QualityBalancedDesc => "AAC or MP3 · 192 kbps", "AAC или MP3 · 192 кбит/с";
    QualityBestDesc => "FLAC or 320 kbps", "FLAC или 320 кбит/с";
}
