# Ayna — Google Play mağaza girişi: Türkçe (`tr-TR`)

> Play Console → *Büyüme → Mağaza varlığı → Ana mağaza girişi → Türkçe* alanlarına kopyalayın.
> Sınırlar kontrol edildi: başlık ≤ 30, kısa açıklama ≤ 80, tam açıklama ≤ 4000 karakter.
> Tüm iddialar koda dayanır (kaynak tablosu: `listing-en.md` → "Claim sources").

## Title

```text
Ayna: AirPlay & Miracast TV
```

(Tam 30 karakter. Önce marka, sonra tanımlayıcı ifade. AirPlay, Miracast, Chromecast, Apple veya
Samsung uygulamanın kendi adı gibi kullanılmaz; başlıkta "ücretsiz", "en iyi", fiyat vb. olmaz.)

## Short description

```text
iPhone, iPad, Mac, Android ve PC ekranını düşük gecikmeyle hızla TV'ye yansıtın
```

## Full description

```text
Android TV veya Google TV'nizi iPhone, iPad, Mac, Android telefon ve Windows bilgisayarınız için hızlı bir kablosuz ekran yansıtma alıcısına dönüştürün — ek donanım veya kablo gerekmez.

RUST İLE YAZILMIŞ HIZLI MOTOR
Ayna sıfırdan, doğrudan çalışan (native) Rust ile yazıldı:
• Görüntü yolunda Java sanal makinesi ya da çöp toplayıcı (GC) duraklaması yoktur.
• TV'nizi yormaz: küçük indirme boyutu, düşük işlemci ve bellek kullanımı.
• Hızlı açılır; açık olduğu sürece alıcıyı arka planda hazır tutar.

DÜŞÜK GECİKME VE 60 FPS
• TV'nizin kendi video işlemcisiyle donanımsal HEVC (H.265) ve H.264 çözme, düşük gecikme ayarlarıyla.
• Sunum, video ve fotoğraflarda akıcı görüntü için 60 fps'ye kadar.
• Görüntü ve ses senkron kalır; gecikme modu ve ses tamponu ayarlanabilir.

ÜÇ PROTOKOL, TEK UYGULAMA
• AirPlay: iPhone, iPad veya Mac ekranınızı yansıtın ya da müzik çalın (Denetim Merkezi → Ekran Yansıtma).
• Miracast: Kablosuz ekran / ekran yansıtma özelliği olan Android telefon ve tabletler Wi-Fi Direct ile doğrudan bağlanır.
• Windows 10 / 11: Win + K ile masaüstünüzü TV'ye kopyalayın veya genişletin.
• DLNA / UPnP: Ağınızdaki DLNA uygulamalarından video ve müzik gönderin (MP4, MKV, WebM, TS, MP3, FLAC, AAC).

NET SES
• AirPlay ile kayıpsız ALAC (44.1 kHz / 16-bit) müzik, ayrıca AAC ve PCM.
• AirPlay ekran yansıtmada AAC-ELD ses.
• Albüm kapağı, şarkı, sanatçı ve ilerleme çubuğu içeren "Şimdi Çalıyor" kartı.

GİZLİLİK ÖNCELİKLİ
• Hesap yok, bulut yok, harici sunucu yok.
• Ekranınız ve medyanız yerel ağınız veya Wi-Fi Direct üzerinden doğrudan TV'nize gider.
• AirPlay bağlantıları için isteğe bağlı ekranda PIN kodu.

KUMANDA İÇİN TASARLANDI
• Kumandayla rahat kullanılan sade arayüz ve canlı durum göstergesi.
• Apple, Android ve Windows için adım adım bağlantı rehberi.
• 10 dil: Türkçe, İngilizce, Almanca, İspanyolca, Fransızca, İtalyanca, Portekizce, Rusça, Lehçe, Endonezce.
• Çözünürlük (1080p / 4K), kare hızı, kodek, gecikme modu ve ses tamponu ayarları.

ÜCRETSİZ VE PRO
Üç protokolün tamamı ücretsiz sürümde çalışır:
• Ücretsiz: Donanımsal HEVC/H.264 ile AirPlay, Miracast ve DLNA, 1080p yansıtma. Reklamlar yalnızca oturumlar arasında gösterilir, içeriğinizin üzerinde asla.
• Ayna Pro: 4K (2160p) yansıtma ve reklamsız kullanım — yıllık abonelik veya ömür boyu tek seferlik satın alma.

AÇIK KAYNAK
Ayna, GNU GPL v3 lisanslı özgür yazılımdır:
https://github.com/krmndigital-lab/yansit-tv

BİLMENİZ GEREKENLER
• TV ve gönderen cihaz aynı Wi-Fi ağında olmalıdır (Miracast, Wi-Fi Direct ile doğrudan bağlanır).
• Google Cast (Chromecast) desteklenmez; Android'de Miracast veya DLNA kullanın.
• 4K için 4K TV, 4K gönderebilen bir cihaz ve Ayna Pro gerekir.

TİCARİ MARKALAR
AirPlay, iPhone, iPad ve Mac, Apple Inc.'in; Miracast, Wi-Fi Alliance'ın; DLNA, Digital Living Network Alliance'ın; Android, Android TV, Google TV ve Chromecast, Google LLC'nin; Windows, Microsoft Corporation'ın ticari markalarıdır. Ayna bağımsız bir uygulamadır ve bu şirketlerle bağlantılı değildir.
```

---

## Mağaza ayarları (genel, bir kez yapılır)

| Ayar | Öneri |
|---|---|
| Kategori | **Video Oynatıcılar ve Düzenleyiciler** (alternatif: Araçlar) |
| Etiketler | Play'in hazır listesinden en yakın olanları seçin (video oynatıcı, yayın/aktarım). Serbest etiket girilemez. |
| İçerik derecelendirmesi (IARC) | Beklenen sonuç **Herkes / PEGI 3 / USK 0**. Uygulama kendi içeriğini üretmez: şiddet, cinsellik, küfür, kumar ya da kullanıcılar arası sohbet yoktur. Yalnızca kullanıcının kendi cihazından gönderdiğini gösterir. Web'de gezinme sunmaz (DLNA, yalnızca kullanıcının kendi gönderen uygulamasının ilettiği bağlantıyı oynatır), bu yüzden "sınırsız internet erişimi" sorusuna **Hayır** deyin. Son derecelendirmeyi IARC verir. |
| Reklam içerir | **Evet** (yalnızca ücretsiz sürümde, oturumlar arasında AdMob geçiş reklamı). Bu yanıt, yüklenen derlemeyle uyumlu olmalıdır. |
| Uygulama içi satın alma | Evet: `yansit_pro_yearly` (abonelik, temel plan `yearly`, 10,00 ABD doları) ve `yansit_pro_lifetime` (tek seferlik, 20,00 ABD doları). Ülke fiyatları Play Console'da belirlenir. |
| Hedef kitle | **Yalnızca 13 yaş ve üzeri gruplar.** 13 yaş altı bir grup seçilirse Aileler politikası devreye girer (sertifikalı reklam ağları, daha sıkı reklam kuralları). |
| Veri güvenliği | `privacy-policy.md` ekine bakın (geliştirici hiçbir veri toplamaz; ücretsiz sürümde AdMob SDK reklam verisi toplar). |
| Uygulama erişimi (inceleme ekibi için) | Giriş gerekmez. İnceleme notu (İngilizce yazın): *"Testing needs a second device on the same Wi-Fi: an iPhone/iPad/Mac (Control Center → Screen Mirroring), a Windows 10/11 PC or Android phone with Miracast, or any DLNA app."* |
| Cihaz türleri | **Android TV** için katılım (Yayın → Gelişmiş ayarlar → Cihaz türleri). TV banner'ı, en az bir TV ekran görüntüsü ve Google'ın TV kalite incelemesi gerekir. |

## Ek bilgiler

- Gizlilik politikası URL'si: `docs/store/privacy-policy.md` sayfasını herkese açık statik bir sayfa
  olarak yayımlayın (ör. GitHub Pages) ve adresi Play Console → *Uygulama içeriği → Gizlilik
  politikası* alanına girin.
- Kaynak kodu (GPL-3.0-or-later): `https://github.com/krmndigital-lab/yansit-tv`. Yukarıdaki "AÇIK KAYNAK" paragrafı,
  GPLv3 §6(d)'nin istediği "nesne kodunun yanında açık yönlendirme"dir; bkz. `SOURCE-OFFER.md`.
- Destek e-postası (Play Console iletişim bilgileri): `krmndigital@gmail.com`.
