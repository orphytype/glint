//! Genre names. The API names a genre by an id ("rusrap", "local-indie"); this turns the ids it
//! is known to use into words in Russian or English, and any other id into a readable guess.

use crate::i18n::Lang;

/// Every genre id we know, with its Russian and English name.
const NAMES: &[(&str, &str, &str)] = &[
    ("pop", "Поп", "Pop"),
    ("ruspop", "Русская поп-музыка", "Russian pop"),
    ("foreignpop", "Зарубежная поп-музыка", "Foreign pop"),
    ("kpop", "K-pop", "K-pop"),
    ("jpop", "J-pop", "J-pop"),
    ("hyperpop", "Гиперпоп", "Hyperpop"),
    ("allrock", "Рок", "Rock"),
    ("rock", "Рок", "Rock"),
    ("rusrock", "Русский рок", "Russian rock"),
    ("ukrrock", "Украинский рок", "Ukrainian rock"),
    ("rnr", "Рок-н-ролл", "Rock and roll"),
    ("hardrock", "Хард-рок", "Hard rock"),
    ("folkrock", "Фолк-рок", "Folk rock"),
    ("stonerrock", "Стоунер-рок", "Stoner rock"),
    ("prog", "Прогрессив", "Progressive"),
    ("postrock", "Пост-рок", "Post-rock"),
    ("newwave", "Новая волна", "New wave"),
    ("alternative", "Альтернатива", "Alternative"),
    ("indie", "Инди", "Indie"),
    ("local-indie", "Русское инди", "Russian indie"),
    ("punk", "Панк", "Punk"),
    ("hardcore", "Хардкор", "Hardcore"),
    ("posthardcore", "Пост-хардкор", "Post-hardcore"),
    ("emo", "Эмо", "Emo"),
    ("metal", "Метал", "Metal"),
    ("numetal", "Ню-метал", "Nu metal"),
    ("epicmetal", "Эпический метал", "Epic metal"),
    ("folkmetal", "Фолк-метал", "Folk metal"),
    ("gothicmetal", "Готик-метал", "Gothic metal"),
    ("alternativemetal", "Альтернативный метал", "Alternative metal"),
    ("classicmetal", "Классический метал", "Classic metal"),
    ("extrememetal", "Экстремальный метал", "Extreme metal"),
    ("rap", "Рэп и хип-хоп", "Hip-hop and rap"),
    ("rusrap", "Русский рэп", "Russian rap"),
    ("foreignrap", "Зарубежный рэп", "Foreign rap"),
    ("phonk", "Фонк", "Phonk"),
    ("electronics", "Электроника", "Electronic"),
    ("edmgenre", "EDM", "EDM"),
    ("dance", "Танцевальная", "Dance"),
    ("eurodance", "Евроданс", "Eurodance"),
    ("house", "Хаус", "House"),
    ("techno", "Техно", "Techno"),
    ("trance", "Транс", "Trance"),
    ("dnb", "Драм-н-бейс", "Drum and bass"),
    ("dubstep", "Дабстеп", "Dubstep"),
    ("industrial", "Индастриал", "Industrial"),
    ("experimental", "Экспериментальная", "Experimental"),
    ("triphop", "Трип-хоп", "Trip-hop"),
    ("idm", "IDM", "IDM"),
    ("ambient", "Эмбиент", "Ambient"),
    ("lofi", "Лоу-фай", "Lo-fi"),
    ("lounge", "Лаунж", "Lounge"),
    ("relax", "Релакс", "Relax"),
    ("newage", "Нью-эйдж", "New age"),
    ("meditation", "Медитативная", "Meditation"),
    ("naturesounds", "Звуки природы", "Nature sounds"),
    ("jazz", "Джаз", "Jazz"),
    ("conjazz", "Современный джаз", "Contemporary jazz"),
    ("tradjazz", "Традиционный джаз", "Traditional jazz"),
    ("jazzrock", "Джаз-рок", "Jazz rock"),
    ("bebop", "Бибоп", "Bebop"),
    ("smoothjazz", "Смус-джаз", "Smooth jazz"),
    ("vocaljazz", "Вокальный джаз", "Vocal jazz"),
    ("bigbands", "Биг-бенды", "Big bands"),
    ("blues", "Блюз", "Blues"),
    ("rnb", "R&B", "R&B"),
    ("soul", "Соул", "Soul"),
    ("funk", "Фанк", "Funk"),
    ("disco", "Диско", "Disco"),
    ("reggae", "Регги", "Reggae"),
    ("dub", "Даб", "Dub"),
    ("ska", "Ска", "Ska"),
    ("folk", "Фолк", "Folk"),
    ("rusfolk", "Русская народная", "Russian folk"),
    ("country", "Кантри", "Country"),
    ("latinfolk", "Латиноамериканская", "Latin"),
    ("latin", "Латиноамериканская", "Latin"),
    ("celtic", "Кельтская", "Celtic"),
    ("balkan", "Балканская", "Balkan"),
    ("caucasian", "Кавказская", "Caucasian"),
    ("eastern", "Восточная", "Eastern"),
    ("african", "Африканская", "African"),
    ("jewish", "Еврейская", "Jewish"),
    ("classical", "Классическая", "Classical"),
    ("classicalmusic", "Классическая", "Classical"),
    ("modern", "Современная классика", "Modern classical"),
    ("opera", "Опера", "Opera"),
    ("vocal", "Вокальная", "Vocal"),
    ("soundtrack", "Саундтреки", "Soundtracks"),
    ("films", "Музыка из фильмов", "Film music"),
    ("tvseries", "Музыка из сериалов", "TV series music"),
    ("animated", "Музыка из мультфильмов", "Cartoon music"),
    ("videogame", "Музыка из игр", "Video game music"),
    ("musical", "Мюзиклы", "Musicals"),
    ("estrada", "Эстрада", "Estrada"),
    ("rusestrada", "Русская эстрада", "Russian estrada"),
    ("shanson", "Шансон", "Chanson"),
    ("romances", "Романсы", "Romances"),
    ("bard", "Авторская песня", "Bard songs"),
    ("children", "Детская", "Children's"),
    ("for_children", "Детская", "Children's"),
    ("fairytales", "Сказки", "Fairy tales"),
    ("spoken", "Разговорный жанр", "Spoken word"),
    ("audiobooks", "Аудиокниги", "Audiobooks"),
    ("podcasts", "Подкасты", "Podcasts"),
    ("holiday", "Праздничная", "Holiday"),
    ("sport", "Для спорта", "Sport"),
    ("other", "Другое", "Other"),
];

/// The name of genre `id` in `lang`: a known one in words, any other made readable ("some-genre"
/// becomes "Some genre").
pub fn name(id: &str, lang: Lang) -> String {
    let key = id.trim().to_ascii_lowercase();
    if let Some((_, ru, en)) = NAMES.iter().find(|(k, _, _)| *k == key) {
        return (if lang == Lang::Ru { *ru } else { *en }).to_owned();
    }
    let words = key.replace(['-', '_'], " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Several genre ids as one line of names, each once.
pub fn names(ids: &[String], lang: Lang) -> String {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        let n = name(id, lang);
        if !n.is_empty() && !out.contains(&n) {
            out.push(n);
        }
    }
    out.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_genres_read_as_words_and_unknown_ones_are_made_readable() {
        assert_eq!(name("rusrap", Lang::Ru), "Русский рэп");
        assert_eq!(name("rusrap", Lang::En), "Russian rap");
        assert_eq!(name("local-indie", Lang::Ru), "Русское инди");
        assert_eq!(name("some-new_genre", Lang::En), "Some new genre");
        assert_eq!(names(&["rock".into(), "allrock".into(), "pop".into()], Lang::Ru), "Рок, Поп");
    }
}
