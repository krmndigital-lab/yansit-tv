//! UI localization: 10 languages, device-language auto-detection and an in-app override.
//!
//! - The device language (BCP-47 tag from `java.util.Locale.getDefault()`, read over JNI in
//!   `android/mod.rs`) picks the UI language on first start; unsupported languages fall back
//!   to English.
//! - Settings → "Dil · Language" stores `LangPref::Auto` (follow the device) or a fixed
//!   language; [`apply`] makes it current.
//! - Every key must have exactly one string per language: the `strings!` macro pattern only
//!   matches 10 entries, so a missing translation is a compile error. Tests check that `{}`
//!   placeholders match across languages and that every glyph exists in the bundled fonts
//!   (no tofu boxes on screen).
//!
//! Only scripts covered by the bundled Inter fonts are offered (Latin + Cyrillic). Arabic
//! needs RTL shaping and CJK needs a large extra font, so they are not included.
//! GPL-3.0-or-later.

use std::sync::atomic::{AtomicU8, Ordering::Relaxed};

pub const COUNT: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Lang {
    Tr = 0,
    En = 1,
    De = 2,
    Es = 3,
    Fr = 4,
    It = 5,
    Pt = 6,
    Ru = 7,
    Pl = 8,
    Id = 9,
}

pub const ALL: [Lang; COUNT] = [
    Lang::Tr,
    Lang::En,
    Lang::De,
    Lang::Es,
    Lang::Fr,
    Lang::It,
    Lang::Pt,
    Lang::Ru,
    Lang::Pl,
    Lang::Id,
];

impl Lang {
    pub fn code(self) -> &'static str {
        ["tr", "en", "de", "es", "fr", "it", "pt", "ru", "pl", "id"][self as usize]
    }

    /// Name of the language in that language (shown in the picker).
    pub fn native_name(self) -> &'static str {
        [
            "Türkçe",
            "English",
            "Deutsch",
            "Español",
            "Français",
            "Italiano",
            "Português",
            "Русский",
            "Polski",
            "Bahasa Indonesia",
        ][self as usize]
    }

    fn from_u8(v: u8) -> Lang {
        ALL.get(v as usize).copied().unwrap_or(Lang::En)
    }

    /// BCP-47 / Java locale tag ("tr-TR", "pt-BR", "in-ID", "en_US") → supported language.
    /// Java reports Indonesian with the legacy code "in"; both "in" and "id" are accepted.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let primary = tag
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        Some(match primary.as_str() {
            "tr" => Lang::Tr,
            "en" => Lang::En,
            "de" => Lang::De,
            "es" => Lang::Es,
            "fr" => Lang::Fr,
            "it" => Lang::It,
            "pt" => Lang::Pt,
            "ru" => Lang::Ru,
            "pl" => Lang::Pl,
            "id" | "in" => Lang::Id,
            _ => return None,
        })
    }
}

/// Language preference stored in the settings: follow the device, or a fixed language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LangPref {
    #[default]
    Auto,
    Fixed(Lang),
}

impl LangPref {
    pub fn to_cfg(self) -> &'static str {
        match self {
            LangPref::Auto => "auto",
            LangPref::Fixed(l) => l.code(),
        }
    }

    pub fn from_cfg(v: &str) -> LangPref {
        if v.trim().eq_ignore_ascii_case("auto") {
            return LangPref::Auto;
        }
        Lang::from_tag(v).map(LangPref::Fixed).unwrap_or(LangPref::Auto)
    }

    /// Settings-row cycle: Auto → Türkçe → English → … → Bahasa Indonesia → Auto.
    pub fn step(self, forward: bool) -> LangPref {
        let n = COUNT + 1;
        let idx = match self {
            LangPref::Auto => 0,
            LangPref::Fixed(l) => l as usize + 1,
        };
        let j = if forward { (idx + 1) % n } else { (idx + n - 1) % n };
        if j == 0 {
            LangPref::Auto
        } else {
            LangPref::Fixed(ALL[j - 1])
        }
    }

    pub fn resolve(self) -> Lang {
        match self {
            LangPref::Auto => device_lang(),
            LangPref::Fixed(l) => l,
        }
    }
}

static DEVICE: AtomicU8 = AtomicU8::new(Lang::En as u8);
static CURRENT: AtomicU8 = AtomicU8::new(Lang::En as u8);

/// Record the device locale (from JNI). Unsupported languages map to English.
pub fn set_device_tag(tag: &str) -> Lang {
    let l = Lang::from_tag(tag).unwrap_or(Lang::En);
    DEVICE.store(l as u8, Relaxed);
    l
}

pub fn device_lang() -> Lang {
    Lang::from_u8(DEVICE.load(Relaxed))
}

/// Make `pref` the active UI language; returns the resolved language.
pub fn apply(pref: LangPref) -> Lang {
    let l = pref.resolve();
    CURRENT.store(l as u8, Relaxed);
    l
}

pub fn current() -> Lang {
    Lang::from_u8(CURRENT.load(Relaxed))
}

/// Localized string for `k` in the current language.
pub fn t(k: K) -> &'static str {
    get(k, current())
}

/// Localized template with `{}` placeholders filled in order.
pub fn tf(k: K, args: &[&str]) -> String {
    fill(t(k), args)
}

pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 24);
    let mut rest = template;
    let mut it = args.iter();
    while let Some(pos) = rest.find("{}") {
        out.push_str(&rest[..pos]);
        out.push_str(it.next().copied().unwrap_or(""));
        rest = &rest[pos + 2..];
    }
    out.push_str(rest);
    out
}

/// Upper-case for kicker labels: Turkish dotted/dotless i rules only in Turkish
/// ("iPhone" must stay "IPHONE" in English, but "işlem" → "İŞLEM" in Turkish).
pub fn upper(s: &str) -> String {
    if current() != Lang::Tr {
        return s.to_uppercase();
    }
    let mut o = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            'i' => o.push('İ'),
            'ı' => o.push('I'),
            c => o.extend(c.to_uppercase()),
        }
    }
    o
}

/// AirPlay receiver name presets for the settings row (first = first-run default).
pub fn device_name_presets(l: Lang) -> &'static [&'static str] {
    match l {
        Lang::Tr => &["Salon TV", "Oturma Odası TV", "Yatak Odası TV", "Mutfak TV", "Sinema Odası", "Ayna TV"],
        Lang::En => &["Living Room TV", "Bedroom TV", "Kitchen TV", "Home Cinema", "Family Room TV", "Ayna TV"],
        Lang::De => &["Wohnzimmer-TV", "Schlafzimmer-TV", "Küchen-TV", "Heimkino", "Kinderzimmer-TV", "Ayna TV"],
        Lang::Es => &["TV del salón", "TV del dormitorio", "TV de la cocina", "Cine en casa", "TV familiar", "Ayna TV"],
        Lang::Fr => &["TV du salon", "TV de la chambre", "TV de la cuisine", "Home cinéma", "TV familiale", "Ayna TV"],
        Lang::It => &["TV del soggiorno", "TV della camera", "TV della cucina", "Home cinema", "TV di famiglia", "Ayna TV"],
        Lang::Pt => &["TV da sala", "TV do quarto", "TV da cozinha", "Home theater", "TV da família", "Ayna TV"],
        Lang::Ru => &["ТВ в гостиной", "ТВ в спальне", "ТВ на кухне", "Домашний кинотеатр", "ТВ в детской", "Ayna TV"],
        Lang::Pl => &["TV w salonie", "TV w sypialni", "TV w kuchni", "Kino domowe", "TV rodzinny", "Ayna TV"],
        Lang::Id => &["TV Ruang Tamu", "TV Kamar Tidur", "TV Dapur", "Home Theater", "TV Keluarga", "Ayna TV"],
    }
}

/// First-run AirPlay name in the device language.
pub fn default_device_name() -> &'static str {
    device_name_presets(device_lang())[0]
}

macro_rules! strings {
    ($($key:ident => [$tr:expr, $en:expr, $de:expr, $es:expr, $fr:expr, $it:expr, $pt:expr, $ru:expr, $pl:expr, $id:expr $(,)?]),* $(,)?) => {
        /// Translation keys (one variant per UI string).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum K { $($key),* }

        /// Every key, for tests.
        pub const ALL_KEYS: &[K] = &[$(K::$key),*];

        pub fn get(k: K, l: Lang) -> &'static str {
            match k {
                $(K::$key => [$tr, $en, $de, $es, $fr, $it, $pt, $ru, $pl, $id][l as usize],)*
            }
        }
    };
}

// Order in every row: tr, en, de, es, fr, it, pt, ru, pl, id.
strings! {
    // ---------------------------------------------------------------- settings panel
    SettingsTitle => ["Alıcı Ayarları", "Receiver Settings", "Empfängereinstellungen", "Ajustes del receptor", "Réglages du récepteur", "Impostazioni ricevitore", "Ajustes do receptor", "Настройки приёмника", "Ustawienia odbiornika", "Pengaturan Penerima"],
    SettingsHint => ["Sol/Sağ veya OK", "Left/Right or OK", "Links/Rechts oder OK", "Izq./Der. u OK", "Gauche/Droite ou OK", "Sinistra/Destra o OK", "Esquerda/Direita ou OK", "Влево/Вправо или OK", "Lewo/Prawo lub OK", "Kiri/Kanan atau OK"],
    RowLanguage => ["Dil", "Language", "Sprache", "Idioma", "Langue", "Lingua", "Idioma", "Язык", "Język", "Bahasa"],
    LangDevice => ["Cihaz dili ({})", "Device language ({})", "Gerätesprache ({})", "Idioma del dispositivo ({})", "Langue de l'appareil ({})", "Lingua del dispositivo ({})", "Idioma do dispositivo ({})", "Язык устройства ({})", "Język urządzenia ({})", "Bahasa perangkat ({})"],
    RowDeviceName => ["Cihaz Adı (AirPlay)", "Device Name (AirPlay)", "Gerätename (AirPlay)", "Nombre del dispositivo (AirPlay)", "Nom de l'appareil (AirPlay)", "Nome dispositivo (AirPlay)", "Nome do dispositivo (AirPlay)", "Имя устройства (AirPlay)", "Nazwa urządzenia (AirPlay)", "Nama Perangkat (AirPlay)"],
    RowResolution => ["Yansıtma Çözünürlüğü", "Mirroring Resolution", "Spiegelungsauflösung", "Resolución de duplicación", "Résolution de recopie", "Risoluzione duplicazione", "Resolução do espelhamento", "Разрешение трансляции", "Rozdzielczość kopii ekranu", "Resolusi Pencerminan"],
    RowFps => ["Maksimum Kare Hızı", "Maximum Frame Rate", "Maximale Bildrate", "Velocidad máxima de fotogramas", "Fréquence d'images max.", "Frequenza fotogrammi max", "Taxa máxima de quadros", "Макс. частота кадров", "Maks. liczba klatek", "Frame Rate Maksimum"],
    RowCodec => ["Video Kodek Desteği", "Video Codec Support", "Video-Codec-Unterstützung", "Códecs de vídeo", "Codecs vidéo", "Codec video supportati", "Codecs de vídeo", "Видеокодеки", "Obsługiwane kodeki wideo", "Dukungan Codec Video"],
    CodecH264Only => ["Yalnızca H.264", "H.264 only", "Nur H.264", "Solo H.264", "H.264 uniquement", "Solo H.264", "Somente H.264", "Только H.264", "Tylko H.264", "Hanya H.264"],
    RowLatency => ["Gecikme Optimizasyonu", "Latency Optimization", "Latenzoptimierung", "Optimización de latencia", "Optimisation de la latence", "Ottimizzazione latenza", "Otimização de latência", "Оптимизация задержки", "Optymalizacja opóźnienia", "Optimasi Latensi"],
    LatencyUltra => ["Ultra Düşük Gecikme", "Ultra-low latency", "Ultraniedrige Latenz", "Latencia ultrabaja", "Latence ultra-faible", "Latenza ultra-bassa", "Latência ultrabaixa", "Сверхнизкая задержка", "Ultraniskie opóźnienie", "Latensi sangat rendah"],
    LatencyStandard => ["Standart Senkron", "Standard sync", "Standard-Synchronisation", "Sincronización estándar", "Synchro standard", "Sincronizzazione standard", "Sincronização padrão", "Стандартная синхронизация", "Standardowa synchronizacja", "Sinkron standar"],
    RowColor => ["Renk Doğruluğu", "Color Accuracy", "Farbgenauigkeit", "Precisión del color", "Fidélité des couleurs", "Precisione colore", "Precisão de cor", "Точность цвета", "Wierność kolorów", "Akurasi Warna"],
    ColorForceSdr => ["BT.709 SDR Zorla", "Force BT.709 SDR", "BT.709 SDR erzwingen", "Forzar BT.709 SDR", "Forcer BT.709 SDR", "Forza BT.709 SDR", "Forçar BT.709 SDR", "Принудительно BT.709 SDR", "Wymuś BT.709 SDR", "Paksa BT.709 SDR"],
    Auto => ["Otomatik", "Automatic", "Automatisch", "Automático", "Automatique", "Automatico", "Automático", "Автоматически", "Automatycznie", "Otomatis"],
    RowScaling => ["Görüntü Ölçekleme", "Picture Scaling", "Bildskalierung", "Escalado de imagen", "Mise à l'échelle", "Ridimensionamento immagine", "Escala da imagem", "Масштабирование", "Skalowanie obrazu", "Skala Gambar"],
    ScaleFill => ["Ekranı Doldur", "Fill screen", "Bildschirm füllen", "Llenar pantalla", "Remplir l'écran", "Riempi schermo", "Preencher tela", "Заполнить экран", "Wypełnij ekran", "Penuhi layar"],
    ScaleFit => ["Orijinal Oran (Sığdır)", "Original ratio (fit)", "Originalformat (einpassen)", "Proporción original (ajustar)", "Format d'origine (ajuster)", "Proporzioni originali (adatta)", "Proporção original (ajustar)", "Исходные пропорции (вписать)", "Oryginalne proporcje (dopasuj)", "Rasio asli (pas)"],
    ScaleFitShort => ["Orijinal (Sığdır)", "Original (fit)", "Original (einpassen)", "Original (ajustar)", "Original (ajuster)", "Originale (adatta)", "Original (ajustar)", "Исходный (вписать)", "Oryginał (dopasuj)", "Asli (pas)"],
    RowAudioBuffer => ["Ses Gecikme Tamponu", "Audio Delay Buffer", "Audio-Verzögerungspuffer", "Búfer de retardo de audio", "Tampon de délai audio", "Buffer ritardo audio", "Buffer de atraso de áudio", "Буфер задержки звука", "Bufor opóźnienia dźwięku", "Buffer Tunda Audio"],
    RowPin => ["Ekranda PIN Kilidi", "On-screen PIN Lock", "PIN-Sperre auf dem Bildschirm", "Bloqueo con PIN en pantalla", "Verrouillage par code PIN", "Blocco PIN su schermo", "Bloqueio por PIN na tela", "PIN-код на экране", "Blokada PIN na ekranie", "Kunci PIN di Layar"],
    PinOn => ["Açık (4 Haneli Kod)", "On (4-digit code)", "Ein (4-stelliger Code)", "Activado (código de 4 dígitos)", "Activé (code à 4 chiffres)", "Attivo (codice a 4 cifre)", "Ativado (código de 4 dígitos)", "Вкл. (4-значный код)", "Wł. (kod 4-cyfrowy)", "Aktif (kode 4 digit)"],
    Off => ["Kapalı", "Off", "Aus", "Desactivado", "Désactivé", "Disattivato", "Desativado", "Выкл.", "Wył.", "Nonaktif"],
    RowAutoWake => ["Bağlanınca Otomatik Aç", "Auto-open on Connect", "Bei Verbindung öffnen", "Abrir al conectar", "Ouvrir à la connexion", "Apri alla connessione", "Abrir ao conectar", "Открывать при подключении", "Otwórz po połączeniu", "Buka Otomatis saat Terhubung"],
    AutoWakeOn => ["Açık (Arka Plandan Uyandır)", "On (wake from background)", "Ein (aus Hintergrund wecken)", "Activado (desde segundo plano)", "Activé (depuis l'arrière-plan)", "Attivo (dallo sfondo)", "Ativado (em segundo plano)", "Вкл. (из фона)", "Wł. (wybudzaj z tła)", "Aktif (bangun dari latar)"],
    ProActive => ["Etkin ✓", "Active ✓", "Aktiv ✓", "Activo ✓", "Actif ✓", "Attivo ✓", "Ativo ✓", "Активно ✓", "Aktywne ✓", "Aktif ✓"],
    ProPerYear => ["{} / yıl", "{} / year", "{} / Jahr", "{} / año", "{} / an", "{} / anno", "{} / ano", "{} / год", "{} / rok", "{} / tahun"],
    ProFree => ["Ücretsiz (reklamlı) — Yükselt", "Free (with ads) — Upgrade", "Kostenlos (mit Werbung) — Upgrade", "Gratis (con anuncios) — Mejorar", "Gratuit (avec pubs) — Passer à Pro", "Gratis (con pubblicità) — Passa a Pro", "Grátis (com anúncios) — Atualizar", "Бесплатно (с рекламой) — Улучшить", "Za darmo (z reklamami) — Ulepsz", "Gratis (dengan iklan) — Tingkatkan"],
    LabelValue => ["{}: {}", "{}: {}", "{}: {}", "{}: {}", "{} : {}", "{}: {}", "{}: {}", "{}: {}", "{}: {}", "{}: {}"],

    // ---------------------------------------------------------------- toasts
    ToastDeviceName => ["Alıcı adı '{}' olarak güncellendi", "Receiver name set to '{}'", "Empfängername auf '{}' geändert", "Nombre del receptor: '{}'", "Nom du récepteur : « {} »", "Nome del ricevitore: '{}'", "Nome do receptor: '{}'", "Имя приёмника: «{}»", "Nazwa odbiornika: '{}'", "Nama penerima diubah menjadi '{}'"],
    ToastFps => ["Maksimum kare hızı: {} FPS", "Max frame rate: {} FPS", "Max. Bildrate: {} FPS", "Fotogramas máx.: {} FPS", "Fréquence max. : {} FPS", "Frequenza max: {} FPS", "Taxa máx.: {} FPS", "Макс. частота: {} FPS", "Maks. klatki: {} FPS", "Frame rate maks: {} FPS"],
    ToastHevcOn => ["HEVC (H.265) donanım çözücü etkinleştirildi", "HEVC (H.265) hardware decoding enabled", "HEVC-(H.265)-Hardwaredecodierung aktiviert", "Decodificación HEVC (H.265) por hardware activada", "Décodage matériel HEVC (H.265) activé", "Decodifica hardware HEVC (H.265) attivata", "Decodificação HEVC (H.265) por hardware ativada", "Аппаратное декодирование HEVC (H.265) включено", "Sprzętowe dekodowanie HEVC (H.265) włączone", "Dekode perangkat keras HEVC (H.265) aktif"],
    ToastH264Only => ["Yalnızca H.264 uyumluluk modu seçildi", "H.264-only compatibility mode selected", "Kompatibilitätsmodus: nur H.264", "Modo de compatibilidad: solo H.264", "Mode compatibilité : H.264 uniquement", "Modalità compatibilità: solo H.264", "Modo de compatibilidade: somente H.264", "Режим совместимости: только H.264", "Tryb zgodności: tylko H.264", "Mode kompatibilitas: hanya H.264"],
    ToastLatencyUltra => ["Ultra düşük gecikme modu etkin", "Ultra-low latency mode on", "Ultraniedrige Latenz aktiv", "Modo de latencia ultrabaja activado", "Mode latence ultra-faible activé", "Modalità latenza ultra-bassa attiva", "Modo de latência ultrabaixa ativado", "Режим сверхнизкой задержки включён", "Tryb ultraniskiego opóźnienia włączony", "Mode latensi sangat rendah aktif"],
    ToastLatencyBalanced => ["Dengeli senkronizasyon modu etkin", "Balanced sync mode on", "Ausgewogene Synchronisation aktiv", "Modo de sincronización equilibrada activado", "Mode synchro équilibrée activé", "Modalità sincronizzazione bilanciata attiva", "Modo de sincronização equilibrada ativado", "Сбалансированная синхронизация включена", "Zrównoważona synchronizacja włączona", "Mode sinkron seimbang aktif"],
    ToastSdrOn => ["BT.709 SDR renk profili zorlanıyor", "Forcing BT.709 SDR color profile", "BT.709-SDR-Farbprofil wird erzwungen", "Forzando perfil de color BT.709 SDR", "Profil couleur BT.709 SDR forcé", "Profilo colore BT.709 SDR forzato", "Perfil de cor BT.709 SDR forçado", "Принудительный цветовой профиль BT.709 SDR", "Wymuszono profil kolorów BT.709 SDR", "Profil warna BT.709 SDR dipaksa"],
    ToastColorAuto => ["Otomatik renk profili etkin", "Automatic color profile on", "Automatisches Farbprofil aktiv", "Perfil de color automático activado", "Profil couleur automatique activé", "Profilo colore automatico attivo", "Perfil de cor automático ativado", "Автоматический цветовой профиль", "Automatyczny profil kolorów włączony", "Profil warna otomatis aktif"],
    ToastPinOn => ["Ekranda 4 haneli PIN kilidi etkin", "On-screen 4-digit PIN lock enabled", "4-stellige PIN-Sperre aktiviert", "Bloqueo con PIN de 4 dígitos activado", "Verrouillage par code PIN à 4 chiffres activé", "Blocco PIN a 4 cifre attivato", "Bloqueio por PIN de 4 dígitos ativado", "Блокировка 4-значным PIN-кодом включена", "Blokada 4-cyfrowym PIN-em włączona", "Kunci PIN 4 digit diaktifkan"],
    ToastPinOff => ["PIN kilidi kapatıldı (Doğrudan bağlantı)", "PIN lock off (direct connection)", "PIN-Sperre aus (direkte Verbindung)", "Bloqueo con PIN desactivado (conexión directa)", "Code PIN désactivé (connexion directe)", "Blocco PIN disattivato (connessione diretta)", "Bloqueio por PIN desativado (conexão direta)", "PIN-код отключён (прямое подключение)", "Blokada PIN wyłączona (połączenie bezpośrednie)", "Kunci PIN nonaktif (koneksi langsung)"],
    ToastAutoWakeOn => ["Bağlantı geldiğinde ekran otomatik açılacak", "The app will open automatically on connection", "Die App öffnet sich bei Verbindung automatisch", "La app se abrirá al recibir una conexión", "L'appli s'ouvrira à chaque connexion", "L'app si aprirà alla connessione", "O app abrirá automaticamente ao conectar", "Приложение откроется при подключении", "Aplikacja otworzy się po połączeniu", "Aplikasi akan terbuka otomatis saat terhubung"],
    ToastAutoWakeOff => ["Arka planda sessiz çalışma modu", "Silent background mode", "Stiller Hintergrundmodus", "Modo silencioso en segundo plano", "Mode silencieux en arrière-plan", "Modalità silenziosa in background", "Modo silencioso em segundo plano", "Тихий фоновый режим", "Cichy tryb w tle", "Mode senyap di latar belakang"],
    ToastBuyYearly => ["Yıllık abonelik başlatılıyor…", "Starting yearly subscription…", "Jahresabo wird gestartet…", "Iniciando suscripción anual…", "Lancement de l'abonnement annuel…", "Avvio abbonamento annuale…", "Iniciando assinatura anual…", "Оформление годовой подписки…", "Uruchamianie subskrypcji rocznej…", "Memulai langganan tahunan…"],
    ToastBuyLifetime => ["Ömür boyu satın alma başlatılıyor…", "Starting lifetime purchase…", "Einmalkauf wird gestartet…", "Iniciando compra de por vida…", "Lancement de l'achat à vie…", "Avvio acquisto a vita…", "Iniciando compra vitalícia…", "Оформление пожизненной покупки…", "Uruchamianie zakupu dożywotniego…", "Memulai pembelian seumur hidup…"],
    ToastRestore => ["Satın alımlar geri yükleniyor…", "Restoring purchases…", "Käufe werden wiederhergestellt…", "Restaurando compras…", "Restauration des achats…", "Ripristino acquisti…", "Restaurando compras…", "Восстановление покупок…", "Przywracanie zakupów…", "Memulihkan pembelian…"],
    Picture => ["Görüntü", "Picture", "Bild", "Imagen", "Image", "Immagine", "Imagem", "Изображение", "Obraz", "Gambar"],

    // ---------------------------------------------------------------- remote hints / footer / header
    HintChange => ["Değiştir", "Change", "Ändern", "Cambiar", "Modifier", "Cambia", "Alterar", "Изменить", "Zmień", "Ubah"],
    HintOption => ["Seçenek", "Option", "Option", "Opción", "Option", "Opzione", "Opção", "Опция", "Opcja", "Opsi"],
    HintBackToVideo => ["Videoya Dön", "Back to video", "Zurück zum Video", "Volver al vídeo", "Retour à la vidéo", "Torna al video", "Voltar ao vídeo", "К видео", "Wróć do wideo", "Kembali ke video"],
    HintSetting => ["Ayar", "Setting", "Einstellung", "Ajuste", "Réglage", "Impostazione", "Ajuste", "Настройка", "Ustawienie", "Pengaturan"],
    Disconnect => ["Bağlantıyı Kes", "Disconnect", "Trennen", "Desconectar", "Déconnecter", "Disconnetti", "Desconectar", "Отключить", "Rozłącz", "Putuskan"],
    HintNavigate => ["Gezin", "Navigate", "Navigieren", "Navegar", "Naviguer", "Naviga", "Navegar", "Навигация", "Nawiguj", "Navigasi"],
    HintExit => ["Çıkış", "Exit", "Beenden", "Salir", "Quitter", "Esci", "Sair", "Выход", "Wyjście", "Keluar"],
    KeyBack => ["GERİ", "BACK", "ZURÜCK", "ATRÁS", "RETOUR", "INDIETRO", "VOLTAR", "НАЗАД", "WSTECZ", "KEMBALI"],
    FooterHw => ["Donanım HEVC/AVC çözücü", "Hardware HEVC/AVC decoding", "Hardware-Decodierung HEVC/AVC", "Decodificación HEVC/AVC por hardware", "Décodage matériel HEVC/AVC", "Decodifica hardware HEVC/AVC", "Decodificação HEVC/AVC por hardware", "Аппаратное декодирование HEVC/AVC", "Sprzętowe dekodowanie HEVC/AVC", "Dekode perangkat keras HEVC/AVC"],
    HeaderSubtitle => ["AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP", "AirPlay 2  •  Miracast (WFD)  •  DLNA / UPnP"],

    // ---------------------------------------------------------------- status pill
    StatusIdle => ["BAĞLANTI BEKLENİYOR", "WAITING FOR CONNECTION", "WARTE AUF VERBINDUNG", "ESPERANDO CONEXIÓN", "EN ATTENTE DE CONNEXION", "IN ATTESA DI CONNESSIONE", "AGUARDANDO CONEXÃO", "ОЖИДАНИЕ ПОДКЛЮЧЕНИЯ", "OCZEKIWANIE NA POŁĄCZENIE", "MENUNGGU KONEKSI"],
    StatusConnecting => ["BAĞLANIYOR…", "CONNECTING…", "VERBINDE…", "CONECTANDO…", "CONNEXION…", "CONNESSIONE…", "CONECTANDO…", "ПОДКЛЮЧЕНИЕ…", "ŁĄCZENIE…", "MENGHUBUNGKAN…"],
    StatusMirroring => ["EKRAN YANSITILIYOR", "MIRRORING SCREEN", "BILDSCHIRM WIRD GESPIEGELT", "DUPLICANDO PANTALLA", "RECOPIE DE L'ÉCRAN", "DUPLICAZIONE SCHERMO", "ESPELHANDO TELA", "ТРАНСЛЯЦИЯ ЭКРАНА", "KOPIOWANIE EKRANU", "MENCERMINKAN LAYAR"],
    StatusAudio => ["SES AKTARIMI", "AUDIO STREAMING", "AUDIO-STREAMING", "TRANSMISIÓN DE AUDIO", "DIFFUSION AUDIO", "STREAMING AUDIO", "TRANSMISSÃO DE ÁUDIO", "ПЕРЕДАЧА ЗВУКА", "STRUMIEŃ AUDIO", "STREAMING AUDIO"],

    // ---------------------------------------------------------------- receiver hero card
    KickerMirrorName => ["BU TELEVİZYONUN YANSITMA ADI", "THIS TV'S MIRRORING NAME", "SPIEGELUNGSNAME DIESES TV", "NOMBRE DE ESTE TV PARA DUPLICAR", "NOM DE RECOPIE DE CE TÉLÉVISEUR", "NOME DI QUESTA TV", "NOME DESTA TV PARA ESPELHAMENTO", "ИМЯ ЭТОГО ТЕЛЕВИЗОРА", "NAZWA TEGO TELEWIZORA", "NAMA TV INI UNTUK PENCERMINAN"],
    ConnectedDevice => ["Bağlı Cihaz: {}", "Connected device: {}", "Verbundenes Gerät: {}", "Dispositivo conectado: {}", "Appareil connecté : {}", "Dispositivo connesso: {}", "Dispositivo conectado: {}", "Подключено: {}", "Połączone urządzenie: {}", "Perangkat terhubung: {}"],
    ConnectedWithModel => ["Bağlı: {} ({})", "Connected: {} ({})", "Verbunden: {} ({})", "Conectado: {} ({})", "Connecté : {} ({})", "Connesso: {} ({})", "Conectado: {} ({})", "Подключено: {} ({})", "Połączono: {} ({})", "Terhubung: {} ({})"],
    HeroTagline => ["Apple, Android ve Windows'tan anında yansıtın", "Mirror instantly from Apple, Android & Windows", "Sofort spiegeln von Apple, Android & Windows", "Duplica al instante desde Apple, Android y Windows", "Recopiez depuis Apple, Android et Windows", "Duplica subito da Apple, Android e Windows", "Espelhe na hora de Apple, Android e Windows", "Трансляция с Apple, Android и Windows", "Kopia ekranu z Apple, Android i Windows", "Cerminkan dari Apple, Android & Windows"],
    StatResolution => ["ÇÖZÜNÜRLÜK", "RESOLUTION", "AUFLÖSUNG", "RESOLUCIÓN", "RÉSOLUTION", "RISOLUZIONE", "RESOLUÇÃO", "РАЗРЕШЕНИЕ", "ROZDZIELCZOŚĆ", "RESOLUSI"],
    StatVideoEngine => ["DONANIM ÇÖZÜCÜ", "HARDWARE DECODER", "HARDWARE-DECODER", "DECODIFICADOR HW", "DÉCODEUR MATÉRIEL", "DECODER HARDWARE", "DECODIFICADOR HW", "АППАРАТНЫЙ ДЕКОДЕР", "DEKODER SPRZĘTOWY", "DEKODER PERANGKAT KERAS"],
    EngineHwBoth => ["HEVC + H.264", "HEVC + H.264", "HEVC + H.264", "HEVC + H.264", "HEVC + H.264", "HEVC + H.264", "HEVC + H.264", "HEVC + H.264", "HEVC + H.264", "HEVC + H.264"],
    EngineHwH264 => ["H.264", "H.264", "H.264", "H.264", "H.264", "H.264", "H.264", "H.264", "H.264", "H.264"],
    StatLatency => ["GECİKME MODU", "LATENCY MODE", "LATENZMODUS", "MODO DE LATENCIA", "MODE DE LATENCE", "MODALITÀ LATENZA", "MODO DE LATÊNCIA", "РЕЖИМ ЗАДЕРЖКИ", "TRYB OPÓŹNIENIA", "MODE LATENSI"],
    LatencyUltraShort => ["Ultra Düşük", "Ultra-low", "Ultraniedrig", "Ultrabaja", "Ultra-faible", "Ultra-bassa", "Ultrabaixa", "Сверхнизкая", "Ultraniskie", "Sangat rendah"],
    StatAudio => ["SES KALİTESİ", "AUDIO QUALITY", "TONQUALITÄT", "CALIDAD DE AUDIO", "QUALITÉ AUDIO", "QUALITÀ AUDIO", "QUALIDADE DE ÁUDIO", "КАЧЕСТВО ЗВУКА", "JAKOŚĆ DŹWIĘKU", "KUALITAS AUDIO"],

    // ---------------------------------------------------------------- connection guide
    GuideKicker => ["BAĞLANTI REHBERİ (APPLE • ANDROID • WINDOWS)", "HOW TO CONNECT (APPLE • ANDROID • WINDOWS)", "SO VERBINDEN SIE (APPLE • ANDROID • WINDOWS)", "CÓMO CONECTAR (APPLE • ANDROID • WINDOWS)", "COMMENT SE CONNECTER (APPLE • ANDROID • WINDOWS)", "COME CONNETTERSI (APPLE • ANDROID • WINDOWS)", "COMO CONECTAR (APPLE • ANDROID • WINDOWS)", "КАК ПОДКЛЮЧИТЬСЯ (APPLE • ANDROID • WINDOWS)", "JAK POŁĄCZYĆ (APPLE • ANDROID • WINDOWS)", "CARA MENGHUBUNGKAN (APPLE • ANDROID • WINDOWS)"],
    GuideAppleDesc => ["Denetim Merkezi > Ekran Yansıtma > bu TV", "Control Center > Screen Mirroring > this TV", "Kontrollzentrum > Bildschirmsynchronisierung", "Centro de control > Duplicar pantalla > este TV", "Centre de contrôle > Recopie de l'écran", "Centro di Controllo > Duplica schermo", "Central de Controle > Espelhamento de Tela", "Пункт управления > Повтор экрана", "Centrum sterowania > Synchronizacja ekranu", "Pusat Kontrol > Pencerminan Layar"],
    GuideAndroidTitle => ["Android (telefon & tablet) — Miracast / DLNA", "Android phone & tablet — Miracast / DLNA", "Android (Handy & Tablet) — Miracast / DLNA", "Android (móvil y tablet) — Miracast / DLNA", "Android (mobile, tablette) — Miracast / DLNA", "Android (smartphone, tablet) — Miracast / DLNA", "Android (celular e tablet) — Miracast / DLNA", "Android (телефон, планшет) — Miracast / DLNA", "Android (telefon, tablet) — Miracast / DLNA", "Android (ponsel & tablet) — Miracast / DLNA"],
    GuideAndroidDesc => ["Hızlı Ayarlar > Smart View / Ekran Yansıtma", "Quick Settings > Smart View / Screen Cast", "Schnelleinstellungen > Smart View / Übertragen", "Ajustes rápidos > Smart View / Enviar pantalla", "Réglages rapides > Smart View / Caster", "Impostazioni rapide > Smart View / Trasmetti", "Config. rápidas > Smart View / Transmitir", "Быстрые настройки > Smart View / Трансляция", "Szybkie ustawienia > Smart View / Przesyłaj", "Setelan Cepat > Smart View / Transmisi Layar"],
    GuideWindowsTitle => ["Windows 10 / 11 — Kablosuz Ekran & DLNA", "Windows 10 / 11 — Wireless Display & DLNA", "Windows 10 / 11 — Drahtlosanzeige & DLNA", "Windows 10 / 11 — Pantalla inalámbrica", "Windows 10 / 11 — Affichage sans fil", "Windows 10 / 11 — Schermo wireless", "Windows 10 / 11 — Tela sem fio e DLNA", "Windows 10 / 11 — беспроводной дисплей", "Windows 10 / 11 — ekran bezprzewodowy", "Windows 10 / 11 — Layar Nirkabel & DLNA"],
    GuideWindowsDesc => ["Win + K veya dosyaya sağ tık > Cihaza yayınla", "Win + K, or right-click a file > Cast to Device", "Win + K oder Rechtsklick > Wiedergabe auf Gerät", "Win + K o clic derecho > Transmitir a dispositivo", "Win + K ou clic droit > Caster sur un appareil", "Win + K o clic destro > Trasmetti a dispositivo", "Win + K ou clique direito > Transmitir", "Win + K или ПКМ > Передать на устройство", "Win + K lub PPM > Rzutuj na urządzenie", "Win + K atau klik kanan > Transmisikan"],

    // ---------------------------------------------------------------- audio player card
    AudioKicker => ["AIRPLAY SES AKTARIMI", "AIRPLAY AUDIO", "AIRPLAY-AUDIO", "AUDIO AIRPLAY", "AUDIO AIRPLAY", "AUDIO AIRPLAY", "ÁUDIO AIRPLAY", "ЗВУК AIRPLAY", "DŹWIĘK AIRPLAY", "AUDIO AIRPLAY"],
    LiveAudio => ["Canlı Ses Akışı", "Live audio stream", "Live-Audiostream", "Audio en directo", "Flux audio en direct", "Audio in diretta", "Áudio ao vivo", "Прямой аудиопоток", "Dźwięk na żywo", "Audio langsung"],
    StatAudioCodec => ["SES KODEK", "AUDIO CODEC", "AUDIO-CODEC", "CÓDEC DE AUDIO", "CODEC AUDIO", "CODEC AUDIO", "CODEC DE ÁUDIO", "АУДИОКОДЕК", "KODEK AUDIO", "CODEC AUDIO"],
    StatVolume => ["SES SEVİYESİ", "VOLUME", "LAUTSTÄRKE", "VOLUMEN", "VOLUME", "VOLUME", "VOLUME", "ГРОМКОСТЬ", "GŁOŚNOŚĆ", "VOLUME"],
    Muted => ["Sessiz", "Muted", "Stumm", "Silenciado", "Muet", "Muto", "Mudo", "Без звука", "Wyciszono", "Bisu"],
    Percent => ["%{}", "{}%", "{} %", "{} %", "{} %", "{}%", "{}%", "{} %", "{}%", "{}%"],
    StatPackets => ["ALINAN PAKET", "PACKETS RECEIVED", "EMPFANGENE PAKETE", "PAQUETES RECIBIDOS", "PAQUETS REÇUS", "PACCHETTI RICEVUTI", "PACOTES RECEBIDOS", "ПОЛУЧЕНО ПАКЕТОВ", "ODEBRANE PAKIETY", "PAKET DITERIMA"],
    RtpPackets => ["{} RTP paketi", "{} RTP packets", "{} RTP-Pakete", "{} paquetes RTP", "{} paquets RTP", "{} pacchetti RTP", "{} pacotes RTP", "{} RTP-пакетов", "{} pakietów RTP", "{} paket RTP"],
    StatNtp => ["AĞ SENKRON (NTP)", "NETWORK SYNC (NTP)", "NETZWERK-SYNC (NTP)", "SINCRONÍA DE RED (NTP)", "SYNCHRO RÉSEAU (NTP)", "SINCRONIA DI RETE (NTP)", "SINCRONIA DE REDE (NTP)", "СЕТЕВАЯ СИНХР. (NTP)", "SYNCHRONIZACJA (NTP)", "SINKRON JARINGAN (NTP)"],
    Locked => ["Kilitlendi", "Locked", "Synchronisiert", "Sincronizado", "Synchronisé", "Sincronizzato", "Sincronizado", "Синхронизировано", "Zsynchronizowano", "Tersinkron"],

    // ---------------------------------------------------------------- mirroring HUD
    HudMirroring => ["AirPlay Ekran Yansıtma", "AirPlay Screen Mirroring", "AirPlay-Bildschirmspiegelung", "Duplicación de pantalla AirPlay", "Recopie d'écran AirPlay", "Duplicazione schermo AirPlay", "Espelhamento de tela AirPlay", "Трансляция экрана AirPlay", "Kopia ekranu AirPlay", "Pencerminan Layar AirPlay"],
    Decoder => ["Çözücü", "Decoder", "Decoder", "Decodificador", "Décodeur", "Decoder", "Decodificador", "Декодер", "Dekoder", "Dekoder"],
    HudRatio => ["Oran", "Ratio", "Format", "Proporción", "Format", "Proporzioni", "Proporção", "Формат", "Proporcje", "Rasio"],
    HudSettings => ["Ayarlar Paneli", "Settings panel", "Einstellungen", "Panel de ajustes", "Réglages", "Impostazioni", "Painel de ajustes", "Настройки", "Ustawienia", "Panel pengaturan"],
    HudHide => ["HUD Gizle", "Hide HUD", "HUD ausblenden", "Ocultar HUD", "Masquer le HUD", "Nascondi HUD", "Ocultar HUD", "Скрыть HUD", "Ukryj HUD", "Sembunyikan HUD"],

    // ---------------------------------------------------------------- PIN modal
    PinKicker => ["AIRPLAY GÜVENLİK KODU", "AIRPLAY SECURITY CODE", "AIRPLAY-SICHERHEITSCODE", "CÓDIGO DE SEGURIDAD AIRPLAY", "CODE DE SÉCURITÉ AIRPLAY", "CODICE DI SICUREZZA AIRPLAY", "CÓDIGO DE SEGURANÇA AIRPLAY", "КОД БЕЗОПАСНОСТИ AIRPLAY", "KOD ZABEZPIECZAJĄCY AIRPLAY", "KODE KEAMANAN AIRPLAY"],
    PinDesc => ["iPad veya iPhone ekranında bu 4 haneli kodu girin", "Enter this 4-digit code on your iPad or iPhone", "Gib diesen 4-stelligen Code auf dem iPad oder iPhone ein", "Introduce este código de 4 dígitos en tu iPad o iPhone", "Saisissez ce code à 4 chiffres sur votre iPad ou iPhone", "Inserisci questo codice a 4 cifre su iPad o iPhone", "Digite este código de 4 dígitos no iPad ou iPhone", "Введите этот 4-значный код на iPad или iPhone", "Wpisz ten 4-cyfrowy kod na iPadzie lub iPhonie", "Masukkan kode 4 digit ini di iPad atau iPhone Anda"],

    // ---------------------------------------------------------------- About sheet
    RowAbout => ["Hakkında", "About", "Über", "Acerca de", "À propos", "Informazioni", "Sobre", "О приложении", "O aplikacji", "Tentang"],
    AboutLicense => ["Özgür yazılımdır; GPL-3.0-or-later lisansıyla dağıtılır. Hiçbir garanti verilmez.", "Free software under the GPL-3.0-or-later licence, provided without any warranty.", "Freie Software unter der GPL-3.0-or-later, ohne jegliche Gewährleistung.", "Software libre bajo la licencia GPL-3.0-or-later, sin ninguna garantía.", "Logiciel libre sous licence GPL-3.0-or-later, fourni sans aucune garantie.", "Software libero con licenza GPL-3.0-or-later, fornito senza alcuna garanzia.", "Software livre sob a licença GPL-3.0-or-later, fornecido sem qualquer garantia.", "Свободное ПО по лицензии GPL-3.0-or-later, предоставляется без каких-либо гарантий.", "Wolne oprogramowanie na licencji GPL-3.0-or-later, bez jakiejkolwiek gwarancji.", "Perangkat lunak bebas berlisensi GPL-3.0-or-later, tanpa jaminan apa pun."],
    AboutSource => ["Kaynak kodu: {}", "Source code: {}", "Quellcode: {}", "Código fuente: {}", "Code source : {}", "Codice sorgente: {}", "Código-fonte: {}", "Исходный код: {}", "Kod źródłowy: {}", "Kode sumber: {}"],
    AboutSourceListing => ["Kaynak kodu bağlantısı uygulamanın Google Play sayfasındadır.", "The source code link is on the app's Google Play page.", "Der Link zum Quellcode steht auf der Google-Play-Seite der App.", "El enlace al código fuente está en la página de Google Play de la app.", "Le lien vers le code source figure sur la page Google Play de l'appli.", "Il link al codice sorgente è nella pagina Google Play dell'app.", "O link do código-fonte está na página do app no Google Play.", "Ссылка на исходный код — на странице приложения в Google Play.", "Link do kodu źródłowego jest na stronie aplikacji w Google Play.", "Tautan kode sumber ada di halaman Google Play aplikasi ini."],
    AboutCredits => ["AirPlay alıcısı UxPlay (GPL/LGPL) ve shairplay-rust (LGPL-3.0) projelerine dayanır. Yazı tipleri: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "The AirPlay receiver builds on UxPlay (GPL/LGPL) and shairplay-rust (LGPL-3.0). Fonts: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "Der AirPlay-Empfänger basiert auf UxPlay (GPL/LGPL) und shairplay-rust (LGPL-3.0). Schriften: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "El receptor AirPlay se basa en UxPlay (GPL/LGPL) y shairplay-rust (LGPL-3.0). Fuentes: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "Le récepteur AirPlay s'appuie sur UxPlay (GPL/LGPL) et shairplay-rust (LGPL-3.0). Polices : Inter (OFL-1.1), Material Symbols (Apache-2.0).", "Il ricevitore AirPlay si basa su UxPlay (GPL/LGPL) e shairplay-rust (LGPL-3.0). Font: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "O receptor AirPlay é baseado no UxPlay (GPL/LGPL) e no shairplay-rust (LGPL-3.0). Fontes: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "Приёмник AirPlay основан на UxPlay (GPL/LGPL) и shairplay-rust (LGPL-3.0). Шрифты: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "Odbiornik AirPlay bazuje na UxPlay (GPL/LGPL) i shairplay-rust (LGPL-3.0). Czcionki: Inter (OFL-1.1), Material Symbols (Apache-2.0).", "Penerima AirPlay dibangun di atas UxPlay (GPL/LGPL) dan shairplay-rust (LGPL-3.0). Font: Inter (OFL-1.1), Material Symbols (Apache-2.0)."],
    AboutTrademark => ["AirPlay, Apple Inc.'in; Miracast, Wi-Fi Alliance'ın ticari markasıdır. Ayna bağımsızdır ve onlarla bağlantılı değildir.", "AirPlay is a trademark of Apple Inc. and Miracast of Wi-Fi Alliance. Ayna is independent and not affiliated with them.", "AirPlay ist eine Marke von Apple Inc., Miracast der Wi-Fi Alliance. Ayna ist unabhängig und nicht mit ihnen verbunden.", "AirPlay es una marca de Apple Inc. y Miracast de Wi-Fi Alliance. Ayna es independiente y no está afiliado a ellas.", "AirPlay est une marque d'Apple Inc. et Miracast de la Wi-Fi Alliance. Ayna est indépendant et n'y est pas affilié.", "AirPlay è un marchio di Apple Inc. e Miracast della Wi-Fi Alliance. Ayna è indipendente e non affiliato.", "AirPlay é marca da Apple Inc. e Miracast da Wi-Fi Alliance. O Ayna é independente e não é afiliado a elas.", "AirPlay — товарный знак Apple Inc., Miracast — Wi-Fi Alliance. Ayna — независимое приложение, не связанное с ними.", "AirPlay jest znakiem towarowym Apple Inc., a Miracast — Wi-Fi Alliance. Ayna jest niezależny i z nimi niezwiązany.", "AirPlay adalah merek dagang Apple Inc. dan Miracast milik Wi-Fi Alliance. Ayna independen dan tidak berafiliasi."],

    // ---------------------------------------------------------------- Ayna Pro upgrade sheet
    UpTitle => ["Ayna Pro ile 4K ve reklamsız deneyim", "Get 4K and an ad-free experience with Ayna Pro", "4K und werbefrei mit Ayna Pro", "4K y sin anuncios con Ayna Pro", "4K et sans publicité avec Ayna Pro", "4K e senza pubblicità con Ayna Pro", "4K e sem anúncios com o Ayna Pro", "4K и без рекламы с Ayna Pro", "4K i bez reklam z Ayna Pro", "4K dan bebas iklan dengan Ayna Pro"],
    UpBenefits => ["4K (2160p) yansıtma  •  Reklam yok  •  Geliştirmeyi destekleyin", "4K (2160p) mirroring  •  No ads  •  Support development", "4K-(2160p)-Spiegelung  •  Keine Werbung  •  Entwicklung unterstützen", "Duplicación 4K (2160p)  •  Sin anuncios  •  Apoya el desarrollo", "Recopie 4K (2160p)  •  Sans publicité  •  Soutenez le développement", "Duplicazione 4K (2160p)  •  Nessuna pubblicità  •  Sostieni lo sviluppo", "Espelhamento 4K (2160p)  •  Sem anúncios  •  Apoie o desenvolvimento", "Трансляция в 4K (2160p)  •  Без рекламы  •  Поддержка разработки", "Kopia ekranu 4K (2160p)  •  Bez reklam  •  Wsparcie rozwoju", "Pencerminan 4K (2160p)  •  Tanpa iklan  •  Dukung pengembangan"],
    UpYearly => ["Yıllık abonelik", "Yearly subscription", "Jahresabo", "Suscripción anual", "Abonnement annuel", "Abbonamento annuale", "Assinatura anual", "Годовая подписка", "Subskrypcja roczna", "Langganan tahunan"],
    UpLifetime => ["Ömür boyu (tek ödeme)", "Lifetime (one-time payment)", "Lebenslang (Einmalzahlung)", "De por vida (pago único)", "À vie (paiement unique)", "A vita (pagamento unico)", "Vitalício (pagamento único)", "Навсегда (разовый платёж)", "Dożywotnio (jednorazowo)", "Seumur hidup (sekali bayar)"],
    UpRestore => ["Satın alımları geri yükle", "Restore purchases", "Käufe wiederherstellen", "Restaurar compras", "Restaurer les achats", "Ripristina acquisti", "Restaurar compras", "Восстановить покупки", "Przywróć zakupy", "Pulihkan pembelian"],
    UpManage => ["Aboneliği yönet", "Manage subscription", "Abo verwalten", "Gestionar suscripción", "Gérer l'abonnement", "Gestisci abbonamento", "Gerenciar assinatura", "Управлять подпиской", "Zarządzaj subskrypcją", "Kelola langganan"],
    UpClose => ["Kapat", "Close", "Schließen", "Cerrar", "Fermer", "Chiudi", "Fechar", "Закрыть", "Zamknij", "Tutup"],
    UpPriceLoading => ["Fiyat yükleniyor…", "Loading price…", "Preis wird geladen…", "Cargando precio…", "Chargement du prix…", "Caricamento prezzo…", "Carregando preço…", "Загрузка цены…", "Wczytywanie ceny…", "Memuat harga…"],
    UpAlreadyPro => ["Ayna Pro etkin — teşekkürler!", "Ayna Pro is active — thank you!", "Ayna Pro ist aktiv — danke!", "Ayna Pro está activo, ¡gracias!", "Ayna Pro est actif — merci !", "Ayna Pro è attivo — grazie!", "O Ayna Pro está ativo — obrigado!", "Ayna Pro активен — спасибо!", "Ayna Pro jest aktywny — dziękujemy!", "Ayna Pro aktif — terima kasih!"],
    UpTerms => ["Abonelik her yıl otomatik yenilenir; Google Play'den istediğiniz zaman iptal edebilirsiniz.", "The subscription renews every year; cancel anytime in Google Play.", "Das Abo verlängert sich jährlich; jederzeit in Google Play kündbar.", "La suscripción se renueva cada año; cancélala cuando quieras en Google Play.", "L'abonnement se renouvelle chaque année ; résiliable à tout moment sur Google Play.", "L'abbonamento si rinnova ogni anno; annullabile in qualsiasi momento su Google Play.", "A assinatura renova todo ano; cancele quando quiser no Google Play.", "Подписка продлевается ежегодно; отменить можно в любой момент в Google Play.", "Subskrypcja odnawia się co roku; anuluj w dowolnym momencie w Google Play.", "Langganan diperpanjang tiap tahun; batalkan kapan saja di Google Play."],
    ReviewTitle => ["Ayna'yı Google Play'de değerlendirin", "Rate Ayna on Google Play", "Ayna bei Google Play bewerten", "Valora Ayna en Google Play", "Évaluez Ayna sur Google Play", "Valuta Ayna su Google Play", "Avalie o Ayna no Google Play", "Оцените Ayna в Google Play", "Oceń Ayna w Google Play", "Ulas Ayna di Google Play"],
    ReviewDesc => ["Yorumunuz, Ayna'yı geliştirmemize ve başkalarının uygulamayı bulmasına yardımcı olur.", "Your review helps us improve Ayna and helps other people find it.", "Ihre Bewertung hilft uns, Ayna zu verbessern, und anderen, die App zu finden.", "Tu reseña nos ayuda a mejorar Ayna y a que otras personas la encuentren.", "Votre avis nous aide à améliorer Ayna et aide d'autres personnes à la trouver.", "La tua recensione ci aiuta a migliorare Ayna e aiuta altre persone a trovarla.", "Sua avaliação nos ajuda a melhorar o Ayna e ajuda outras pessoas a encontrá-lo.", "Ваш отзыв помогает нам улучшать Ayna, а другим людям — найти приложение.", "Twoja opinia pomaga nam ulepszać Ayna, a innym osobom — znaleźć aplikację.", "Ulasan Anda membantu kami menyempurnakan Ayna dan membantu orang lain menemukannya."],
    ReviewRate => ["Google Play'i aç", "Open Google Play", "Google Play öffnen", "Abrir Google Play", "Ouvrir Google Play", "Apri Google Play", "Abrir o Google Play", "Открыть Google Play", "Otwórz Google Play", "Buka Google Play"],
    ReviewLater => ["Şimdi değil", "Not now", "Nicht jetzt", "Ahora no", "Pas maintenant", "Non ora", "Agora não", "Не сейчас", "Nie teraz", "Nanti saja"],
    ToastRateThanks => ["Teşekkürler!", "Thank you!", "Vielen Dank!", "¡Gracias!", "Merci !", "Grazie!", "Obrigado!", "Спасибо!", "Dziękujemy!", "Terima kasih!"],
    HeaderFree => ["ÜCRETSİZ", "FREE", "GRATIS", "GRATIS", "GRATUIT", "GRATIS", "GRÁTIS", "БЕСПЛАТНО", "BEZPŁATNA", "GRATIS"],
    RowProHighlight => ["Ayna Pro — 4K & Reklamsız", "Ayna Pro — 4K & Ad-Free", "Ayna Pro — 4K & Werbefrei", "Ayna Pro — 4K y Sin Anuncios", "Ayna Pro — 4K et Sans Pub", "Ayna Pro — 4K e Senza Pubblicità", "Ayna Pro — 4K e Sem Anúncios", "Ayna Pro — 4K и Без Рекламы", "Ayna Pro — 4K i Bez Reklam", "Ayna Pro — 4K & Bebas Iklan"],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tags_map_to_languages() {
        assert_eq!(Lang::from_tag("tr-TR"), Some(Lang::Tr));
        assert_eq!(Lang::from_tag("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::from_tag("in-ID"), Some(Lang::Id)); // legacy Java code
        assert_eq!(Lang::from_tag("id"), Some(Lang::Id));
        assert_eq!(Lang::from_tag("en_US"), Some(Lang::En));
        assert_eq!(Lang::from_tag("zh-Hans-CN"), None);
        assert_eq!(Lang::from_tag(""), None);
    }

    #[test]
    fn pref_cycles_through_auto_and_all_languages() {
        let mut p = LangPref::Auto;
        let mut seen = Vec::new();
        for _ in 0..=COUNT {
            p = p.step(true);
            seen.push(p);
        }
        assert_eq!(seen.last(), Some(&LangPref::Auto));
        assert_eq!(LangPref::Auto.step(false), LangPref::Fixed(Lang::Id));
        for l in ALL {
            assert_eq!(LangPref::from_cfg(LangPref::Fixed(l).to_cfg()), LangPref::Fixed(l));
        }
        assert_eq!(LangPref::from_cfg("auto"), LangPref::Auto);
        assert_eq!(LangPref::from_cfg("xx"), LangPref::Auto);
    }

    #[test]
    fn every_translation_has_matching_placeholders() {
        for &k in ALL_KEYS {
            let base = get(k, Lang::Tr).matches("{}").count();
            for l in ALL {
                let s = get(k, l);
                assert!(!s.trim().is_empty(), "{k:?} empty in {l:?}");
                assert_eq!(s.matches("{}").count(), base, "{k:?} placeholder count in {l:?}: {s}");
            }
        }
    }

    #[test]
    fn fill_replaces_in_order() {
        assert_eq!(fill("{}: {} ({})", &["a", "b", "c"]), "a: b (c)");
        assert_eq!(fill("no args", &[]), "no args");
        assert_eq!(fill("{} and {}", &["x"]), "x and ");
    }

    #[test]
    fn upper_respects_language() {
        CURRENT.store(Lang::En as u8, Relaxed);
        assert_eq!(upper("iPhone"), "IPHONE");
        CURRENT.store(Lang::Tr as u8, Relaxed);
        assert_eq!(upper("işlem"), "İŞLEM");
        CURRENT.store(Lang::En as u8, Relaxed);
    }

    /// Minimal TrueType cmap reader (formats 4 and 12) to prove every glyph we render exists.
    fn covered(path: &str) -> HashSet<u32> {
        let d = std::fs::read(path).expect("font file");
        let be16 = |o: usize| u16::from_be_bytes([d[o], d[o + 1]]) as usize;
        let be32 = |o: usize| u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as usize;
        let num = be16(4);
        let mut cmap = 0;
        for i in 0..num {
            let r = 12 + 16 * i;
            if &d[r..r + 4] == b"cmap" {
                cmap = be32(r + 8);
            }
        }
        assert!(cmap > 0, "no cmap in {path}");
        let mut cps = HashSet::new();
        for i in 0..be16(cmap + 2) {
            let s = cmap + be32(cmap + 4 + 8 * i + 4);
            match be16(s) {
                4 => {
                    let seg = be16(s + 6) / 2;
                    for j in 0..seg {
                        let end = be16(s + 14 + 2 * j);
                        let start = be16(s + 16 + 2 * seg + 2 * j);
                        if start != 0xFFFF {
                            cps.extend(start as u32..=end as u32);
                        }
                    }
                }
                12 => {
                    for g in 0..be32(s + 12) {
                        let o = s + 16 + 12 * g;
                        cps.extend(be32(o) as u32..=be32(o + 4) as u32);
                    }
                }
                _ => {}
            }
        }
        cps
    }

    #[test]
    fn every_glyph_is_in_the_bundled_fonts() {
        let root = env!("CARGO_MANIFEST_DIR");
        let mut cps = covered(&format!("{root}/assets/fonts/InterVariable-om.ttf"));
        cps.extend(covered(&format!("{root}/assets/fonts/InterVariable-full.ttf")));
        let mut texts: Vec<&str> = Vec::new();
        for &k in ALL_KEYS {
            for l in ALL {
                texts.push(get(k, l));
            }
        }
        for l in ALL {
            texts.push(l.native_name());
            texts.extend(device_name_presets(l).iter().copied());
        }
        for s in texts {
            for c in s.chars() {
                assert!(cps.contains(&(c as u32)), "glyph {c:?} (U+{:04X}) missing for {s:?}", c as u32);
            }
        }
    }
}
