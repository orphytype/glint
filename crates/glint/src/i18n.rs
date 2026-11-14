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
        Lang::En
    }

    pub fn tracks(self, n: usize) -> String {
        match self {
            Lang::En => format!("{n} {}", if n == 1 { "track" } else { "tracks" }),
            Lang::Ru => format!("{n} {}", plural_ru(n, "трек", "трека", "треков")),
        }
    }

    /// "3 more artists" / "ещё 3 исполнителя": the button after the first names of a track.
    pub fn more_artists(self, n: usize) -> String {
        match self {
            Lang::En => format!("{n} more"),
            Lang::Ru => format!("ещё {n} {}", plural_ru(n, "исполнитель", "исполнителя", "исполнителей")),
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

    /// The confirmation copy for a batch download, including the correctly inflected file count.
    pub fn download_confirm_note(self, n: usize) -> String {
        match self {
            Lang::En => format!("{n} {} will be kept separately from the playback cache.", if n == 1 { "file" } else { "files" }),
            Lang::Ru => {
                let verb = if n % 10 == 1 && n % 100 != 11 { "будет" } else { "будут" };
                format!("{n} {} {verb} храниться отдельно от кэша воспроизведения.", plural_ru(n, "файл", "файла", "файлов"))
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

#[cfg(windows)]
fn windows_locale() -> Option<Lang> {
    // `GetUserDefaultLocaleName` lives in kernel32; ask for it by hand to avoid a feature flag.
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
    }
    let mut buf = [0u16; 85];
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
    Forward => "Forward", "Вперёд";
    ScrollPrev => "Previous", "Предыдущие";
    ScrollNext => "Next", "Следующие";
    Search => "Search", "Поиск";
    Liked => "Liked", "Мне нравится";
    Settings => "Settings", "Настройки";
    Queue => "Queue", "Очередь";
    Lyrics => "Lyrics", "Текст";
    Back => "Back", "Назад";
    Close => "Close", "Закрыть";
    // greetings
    // home
    Play => "Play", "Слушать";
    Pause => "Pause", "Пауза";
    Shuffle => "Shuffle", "Вперемешку";
    Repeat => "Repeat", "Повтор";
    RepeatOff => "Off", "Выкл.";
    RepeatAll => "All", "Все";
    RepeatOne => "One", "Один";
    Next => "Next", "Далее";
    Previous => "Previous", "Назад";
    Like => "Like", "Нравится";
    Unlike => "Remove like", "Убрать лайк";
    Dislike => "Dislike", "Не нравится";
    Disliked => "We'll play less like this", "Будем ставить меньше такого";
    Profile => "Profile", "Профиль";
    ShelfPersonal => "Made for you", "Для вас";
    ShelfEditorial => "Fresh picks", "Подборки";
    ShelfNew => "New releases", "Новые релизы";
    ShelfChart => "Chart", "Чарт";
    Nothing => "Nothing here yet", "Здесь пока пусто";
    Loading => "Loading…", "Загрузка…";
    Retry => "Try again", "Повторить";
    // search
    SearchHint => "Songs, artists, albums, playlists…", "Треки, исполнители, альбомы, плейлисты…";
    Tracks => "Tracks", "Треки";
    Albums => "Albums", "Альбомы";
    Artists => "Artists", "Исполнители";
    Playlists => "Playlists", "Плейлисты";
    NoResults => "No results", "Ничего не найдено";
    TopResult => "Top result", "Лучший результат";
    ShowAll => "Show all", "Показать все";
    Browse => "Browse by mood", "Музыка под настроение";
    Track => "Track", "Трек";
    LikedTracks => "Liked tracks", "Мне нравится";
    MonthlyListeners => "Monthly listeners", "Слушателей в месяц";
    RecentSearches => "Recent searches", "Недавние запросы";
    ListeningHistory => "Listening history", "История прослушивания";
    Download => "Download", "Скачать";
    DownloadAll => "Download all", "Скачать всё";
    DownloadConfirm => "Download all tracks?", "Скачать все треки?";
    DeleteDownload => "Delete download", "Удалить скачанное";
    DeleteDownloadConfirm => "Delete downloaded track?", "Удалить скачанный трек?";
    DeleteDownloadNote => "The downloaded file and its offline data will be removed.", "Скачанный файл и офлайн-данные будут удалены.";
    DownloadDeleted => "Download removed", "Скачанное удалено";
    DownloadDeleteFailed => "Couldn't remove the download", "Не удалось удалить скачанное";
    DownloadInProgress => "Downloading", "Скачивается";
    DownloadDone => "Downloaded", "Скачано";
    DownloadFailed => "Download failed", "Не удалось скачать";
    DownloadsClear => "Delete downloaded tracks", "Удалить скачанные треки";
    DownloadsCleared => "Downloaded tracks deleted", "Скачанные треки удалены";
    ClearHistory => "Clear history", "Очистить историю";
    Forget => "Remove from history", "Удалить из истории";
    AddToCollection => "Add to collection", "Добавить в коллекцию";
    RemoveFromCollection => "Remove from collection", "Убрать из коллекции";
    AddedToCollection => "Added to your collection", "Добавлено в коллекцию";
    RemovedFromCollection => "Removed from your collection", "Убрано из коллекции";
    Share => "Share", "Поделиться";
    LinkCopied => "Link copied", "Ссылка скопирована";
    Details => "Details", "Подробнее";
    WaveByTrack => "My Wave by track", "Моя волна по треку";
    WaveByArtist => "My Wave by artist", "Моя волна по исполнителю";
    WaveByRelease => "My Wave by release", "Моя волна по релизу";
    WaveByCollection => "My Wave by collection", "Моя волна по коллекции";
    ByTrack => "By track", "По треку";
    ByArtist => "By artist", "По исполнителю";
    ByRelease => "By release", "По релизу";
    MoreWaves => "More waves", "Другие волны";
    StageLook => "Player look", "Вид плеера";
    StageFlash => "Flash", "Вспышка";
    StageReact => "Reaction", "Реакция";
    StageOff => "Off", "Нет";
    StageSoft => "Subtle", "Мягкая";
    StageBright => "Vibrant", "Яркая";
    StageCalm => "Calm", "Спокойно";
    StageLively => "Lively", "Живо";
    StageWild => "Wild", "Бешено";
    StageReset => "Reset", "Сбросить";
    StageBack => "Background", "Фон";
    Listen => "Listen", "Слушать";
    Pin => "Pin", "Закрепить";
    Pinned => "Pinned", "Закреплено";
    Unpinned => "Unpin", "Открепить";
    Open => "Open", "Открыть";
    DownloadManager => "Downloads", "Загрузки";
    OpenDownloads => "Open downloads folder", "Открыть папку загрузок";
    ClearDownloadHistory => "Clear download history", "Очистить историю загрузок";
    DownloadAllLiked => "Download liked songs", "Скачать понравившиеся треки";
    LyricAlign => "Alignment", "Выравнивание";
    AlignLeft => "Left", "Слева";
    AlignCenter => "Centre", "По центру";
    AlignRight => "Right", "Справа";
    Mood => "Mood", "Настроение";
    WaveNowPlaying => "Playing now", "Сейчас играет";
    LyricText => "Lyrics", "Текст";
    LyricSize => "Size", "Размер";
    LyricAnim => "Animation", "Анимация";
    LyricFocus => "Focus", "Фокус";
    Smaller => "Smaller", "Мельче";
    Normal => "Normal", "Обычный";
    Larger => "Larger", "Крупнее";
    AnimNone => "None", "Нет";
    AnimSmooth => "Smooth", "Плавно";
    AnimSpring => "Spring", "Пружина";
    FocusLight => "Light", "Слабый";
    FocusMid => "Medium", "Средний";
    FocusStrong => "Strong", "Сильный";
    SearchCollection => "Search the collection", "Поиск в коллекции";
    Resume => "Resume", "Продолжить";
    CopyTitle => "Copy title", "Скопировать название";
    TitleCopied => "Title copied", "Название скопировано";
    CopyLink => "Copy link", "Скопировать ссылку";
    Explicit => "Explicit", "Ненормативная лексика";
    Yes => "Yes", "Да";
    OpenInBrowser => "Open in Yandex Music", "Открыть в Яндекс Музыке";
    PlayAllNext => "Play next", "Играть следующими";
    // library
    PopularTracks => "Popular tracks", "Популярные треки";
    AllTracks => "All tracks", "Все треки";
    All => "All", "Все";
    SetLook => "Look", "Внешний вид";
    SetPlayback => "Playback", "Воспроизведение";
    SetDesktop => "On the desktop", "На рабочем столе";
    Year => "Year", "Год";
    Genre => "Genre", "Жанр";
    Genres => "Genres", "Жанры";
    Duration => "Duration", "Длительность";
    ReleaseDate => "Release date", "Дата релиза";
    Version => "Version", "Версия";
    Description => "Description", "Описание";
    OpenCover => "Open the cover", "Открыть обложку";
    NoLyricsShort => "No lyrics for this track", "У этого трека нет текста";
    GoodMorning => "Good morning", "Доброе утро";
    GoodAfternoon => "Good afternoon", "Добрый день";
    GoodEvening => "Good evening", "Добрый вечер";
    GoodNight => "Good night", "Доброй ночи";
    Discography => "Discography", "Дискография";
    SimilarArtists => "Similar artists", "Похожие исполнители";
    // wave
    WaveTitle => "My Wave", "Моя волна";
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
    NoLyrics => "No lyrics for this track", "Для этого трека нет текста";
    Instrumental => "Instrumental", "Инструментал";
    LyricsLoading => "Finding lyrics…", "Ищем текст…";
    PlayNext => "Play next", "Играть следующим";
    AddToQueue => "Add to queue", "Добавить в очередь";
    RemoveFromQueue => "Remove from queue", "Убрать из очереди";
    GoToAlbum => "Go to album", "Перейти к альбому";
    GoToArtist => "Go to artist", "Перейти к исполнителю";
    QueueEmpty => "The queue is empty", "Очередь пуста";
    // login
    Welcome => "Welcome to Glint", "Добро пожаловать в Glint";
    LoginLead => "An unofficial Yandex Music client. Sign in with your own account or open the demo.", "Неофициальный клиент Яндекс Музыки. Войдите своим аккаунтом или откройте демо.";
    LoginStep2 => "Paste the address you land on, or just the token", "Вставьте адрес, на который попадёте, или сам токен";
    OpenYandex => "Open Yandex sign-in", "Открыть вход Яндекса";
    TokenHint => "Paste the redirect URL or token", "Вставьте адрес или токен";
    SignIn => "Sign in", "Войти";
    SigningIn => "Signing in…", "Входим…";
    TryDemo => "Try the demo", "Попробовать демо";
    DemoNote => "Demo mode plays music synthesised on your machine. No account needed.", "Демо-режим играет музыку, синтезированную на вашем компьютере. Аккаунт не нужен.";
    BadToken => "That doesn't look like a token. Copy the whole address after signing in.", "Это не похоже на токен. Скопируйте весь адрес после входа.";
    TokenRejected => "Yandex rejected that token.", "Яндекс отклонил этот токен.";
    PrivacyNote => "Your token is stored encrypted for this Windows user and is only ever sent to Yandex.", "Токен хранится в зашифрованном виде для вашего пользователя Windows и отправляется только Яндексу.";
    SignOut => "Sign out", "Выйти";
    DemoMode => "Demo mode", "Демо-режим";
    // settings
    Theme => "Theme", "Тема";
    ThemeDark => "Dark", "Тёмная";
    ThemeLight => "Light", "Светлая";
    ThemeSystem => "System", "Системная";
    Accent => "Accent colour", "Цвет акцента";
    AccentAdaptive => "Follow the cover", "Как у обложки";
    Effects => "Visual effects", "Визуальные эффекты";
    EffectsFull => "Full", "Полные";
    EffectsLight => "Light", "Лёгкие";
    EffectsOff => "Off", "Выкл.";
    EffectsNote => "Glass, soft blur and the moving shapes on Home.", "Стекло, плавное размытие и живые фигуры на главной.";
    ReduceMotion => "Reduce motion", "Меньше анимации";
    Language => "Language", "Язык";
    Quality => "Audio quality", "Качество звука";
    QualitySaver => "Saver", "Экономия";
    QualityBalanced => "Balanced", "Баланс";
    QualityBest => "Best", "Лучшее";
    QualityShortSaver => "Eco", "Эконом";
    QualityShortBalanced => "HQ", "Баланс";
    QualityShortBest => "Max", "Макс.";
    PlayerLayout => "Player layout", "Вид плеера";
    PlayerLayoutAuto => "Automatic", "Автоматически";
    PlayerLayoutCompact => "Compact", "Компактный";
    PlayerLayoutComfortable => "Comfortable", "Свободный";
    Equalizer => "Equaliser", "Эквалайзер";
    EqEnabled => "Enable equaliser", "Включить эквалайзер";
    EqReset => "Reset", "Сбросить";
    TrayIcon => "Tray icon", "Значок в трее";
    TrayIconNote => "Keeps the player a click away next to the clock, with Play, Next and Quit in its menu.", "Плеер всегда под рукой рядом с часами: в меню значка есть пауза, переключение треков и выход.";
    CloseToTray => "Close to the tray", "Закрывать в трей";
    CloseToTrayNote => "The close button only hides the window and the music keeps playing. Quit from the tray menu.", "Кнопка «закрыть» только прячет окно, музыка продолжает играть. Выйти можно из меню значка.";
    TrayShow => "Open Glint", "Открыть Glint";
    TrayQuit => "Quit", "Выйти";
    Cache => "Offline data cache", "Кэш данных офлайн";
    CacheImages => "Cover images", "Обложки";
    CacheAudio => "Downloaded audio", "Скачанное аудио";
    CacheMetadata => "Track metadata", "Метаданные треков";
    CacheTotal => "All offline data", "Все офлайн-данные";
    CacheClear => "Clear images", "Очистить обложки";
    CacheClearAll => "Clear all", "Очистить всё";
    AboutText => "An unofficial open-source client for Yandex Music. Not affiliated with or endorsed by Yandex.", "Неофициальный клиент Яндекс Музыки с открытым исходным кодом. Не связан с Яндексом и не одобрен им.";
    // toasts & errors
    Offline => "Can't reach Yandex Music", "Нет связи с Яндекс Музыкой";
    NotAvailable => "This track isn't available in your region", "Трек недоступен в вашем регионе";
    PlaybackFailed => "Couldn't play this track", "Не удалось воспроизвести трек";
    NoAudioDevice => "No audio output device, playing silently", "Нет устройства вывода звука, играем без звука";
    SignedOut => "Signed out", "Вы вышли";
    QueuedToast => "Added to the queue", "Добавлено в очередь";
    PlayNextToast => "Playing next", "Сыграет следующим";
    CacheCleared => "Images cleared", "Обложки очищены";
    CacheAllCleared => "Offline data cleared", "Офлайн-данные очищены";
    // misc
    Album => "Album", "Альбом";
    Playlist => "Playlist", "Плейлист";
    Artist => "Artist", "Исполнитель";
    Plus => "Plus", "Плюс";
    More => "More", "Ещё";
    Mute => "Mute", "Без звука";
    Volume => "Volume", "Громкость";
    ShortcutsText => "Space play/pause · ←/→ seek · Ctrl+←/→ previous/next · ↑/↓ volume · L like · M mute · F now playing · Esc back", "Пробел пауза · ←/→ перемотка · Ctrl+←/→ трек · ↑/↓ громкость · L лайк · M без звука · F экран плеера · Esc назад";
    Flat => "Flat", "Ровно";
    BassBoost => "Bass boost", "Больше баса";
    Vocal => "Vocal", "Вокал";
    Bright => "Bright", "Яркий";
    ClearSearch => "Clear", "Очистить";
    Credits => "Written in Rust with the RUNE UI framework.", "Написано на Rust с UI-фреймворком RUNE.";
    Updates => "Updates", "Обновления";
    AutoUpdate => "Check for updates", "Проверять обновления";
    AutoUpdateNote => "Looks at the GitHub releases when Glint starts. Nothing is installed without your say.", "Смотрит релизы на GitHub при запуске. Без вашего согласия ничего не ставится.";
    CheckNow => "Check now", "Проверить";
    UpdateChecking => "Checking…", "Проверяю…";
    UpToDate => "You have the latest version", "У вас последняя версия";
    UpdateAvailable => "Update available:", "Доступно обновление:";
    UpdateInstall => "Update and restart", "Обновить и перезапустить";
    UpdateReleasePage => "Release page", "Страница релиза";
    UpdateDownloading => "Downloading", "Загрузка";
    UpdateInstalling => "Installing…", "Устанавливаю…";
    UpdateFailed => "Could not update. Try again, or download the new version from GitHub.", "Не удалось обновить. Попробуйте ещё раз или скачайте новую версию с GitHub.";
    UpdateManual => "Installed from a package or built from source: download the new version from the release page.", "Установлено из пакета или собрано из исходников: скачайте новую версию на странице релиза.";
    UpdateToast => "A new version of Glint is available (Settings)", "Доступна новая версия Glint (в настройках)";
    SourceCode => "Source code", "Исходный код";
    EqPreset => "Preset", "Пресет";
    Shortcuts => "Keyboard", "Клавиатура";
    License => "GPL-3.0 licence", "Лицензия GPL-3.0";
    WordLyrics => "Word-by-word lyrics", "Текст по словам";
    WordLyricsNote => "Lights up each word as it is sung, using lrc.red. Switching it on sends the title and artist of each track to lrc.red.", "Подсвечивает каждое слово, пока оно звучит, по данным lrc.red. Включив, вы отправляете lrc.red название и исполнителя каждого трека.";
    FckCensor => "FckCensor", "FckCensor";
    FckCensorNote => "Uses a clean version when FkCensorData has one for the track. Enabled by default.", "Если для трека есть чистая версия в FkCensorData, использует её. Включено по умолчанию.";
    LyricsBy => "Lyrics by lrc.red", "Текст: lrc.red";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_artists_takes_the_russian_plural() {
        assert_eq!(Lang::Ru.more_artists(1), "ещё 1 исполнитель");
        assert_eq!(Lang::Ru.more_artists(3), "ещё 3 исполнителя");
        assert_eq!(Lang::Ru.more_artists(5), "ещё 5 исполнителей");
        assert_eq!(Lang::Ru.more_artists(12), "ещё 12 исполнителей");
        assert_eq!(Lang::Ru.more_artists(22), "ещё 22 исполнителя");
        assert_eq!(Lang::En.more_artists(2), "2 more");
    }

    #[test]
    fn russian_plurals() {
        let ru = Lang::Ru;
        assert_eq!(ru.tracks(1), "1 трек");
        assert_eq!(ru.tracks(2), "2 трека");
        assert_eq!(ru.tracks(5), "5 треков");
        assert_eq!(ru.tracks(11), "11 треков");
        assert_eq!(ru.tracks(21), "21 трек");
        assert_eq!(ru.tracks(112), "112 треков");
        assert_eq!(Lang::En.tracks(1), "1 track");
    }

    #[test]
    fn download_confirmation_uses_file_plural() {
        assert_eq!(Lang::Ru.download_confirm_note(1), "1 файл будет храниться отдельно от кэша воспроизведения.");
        assert_eq!(Lang::Ru.download_confirm_note(1165), "1165 файлов будут храниться отдельно от кэша воспроизведения.");
        assert_eq!(Lang::En.download_confirm_note(2), "2 files will be kept separately from the playback cache.");
    }

    #[test]
    fn language_codes() {
        assert_eq!(Lang::from_code("ru-RU"), Some(Lang::Ru));
        assert_eq!(Lang::from_code("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_code("de-DE"), None);
        assert_eq!(Lang::from_code("r"), None);
    }

    #[test]
    fn a_release_date_is_a_day_to_read() {
        assert_eq!(release_date("2026-11-13T00:00:00+03:00", Lang::En), "13 November 2026");
        assert_eq!(release_date("2026-11-13T00:00:00+03:00", Lang::Ru), "13 ноября 2026");
        assert_eq!(release_date("2026-01-02", Lang::En), "2 January 2026");
        // A date in another shape is left as it is.
        assert_eq!(release_date("2026", Lang::En), "2026");
        assert_eq!(release_date("whenever", Lang::En), "whenever");
    }

    #[test]
    fn durations() {
        assert_eq!(Lang::En.duration(65 * 60), "1 h 05 min");
        assert_eq!(Lang::Ru.duration(12 * 60), "12 мин");
    }
}
