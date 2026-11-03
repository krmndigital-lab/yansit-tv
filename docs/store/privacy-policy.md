<!--
PUBLISHING NOTES (hidden on the rendered page):
- Fill in KRMN Digital, krmndigital@gmail.com and https://github.com/krmndigital-lab/yansit-tv first.
- Host this file as a public static page, e.g. GitHub Pages (Settings → Pages → deploy from branch, folder /docs):
  https://<user>.github.io/<repo>/store/privacy-policy.html  (GitHub Pages renders .md without front matter).
- Play needs a public, non-PDF, non-geo-blocked URL that opens without a login. Paste it in
  Play Console → App content → Privacy policy, and link it from the app's About screen.
- Keep this page in sync with the AndroidManifest permissions and with the SDKs in the uploaded build.
-->

# Ayna Privacy Policy

**Effective date:** 3 November 2026  
**App:** Ayna (`com.yansit.tv`) for Android TV and Google TV  
**Developer:** KRMN Digital · **Contact:** krmndigital@gmail.com

[Türkçe sürüm aşağıdadır.](#turkce)

## Summary

- Ayna has **no account, no sign-up and no servers**. We, the developer, do not receive any data
  from the app.
- Screen mirroring and media playback happen **on your local network**, directly between your own
  devices.
- The **free version shows ads from Google AdMob**. Google may collect your device's advertising
  ID and other device information to serve those ads. **Ayna Pro has no ads.**
- Purchases are processed by **Google Play Billing**.

## 1. Information the app handles on your TV

Everything below stays on your TV:

- **Content you send**: screen image and sound, music and its metadata (title, artist, cover
  art), and DLNA media. It is processed in memory for live playback only. It is not recorded, saved
  or uploaded.
- **Name of the connected device**: shown on screen during a session.
- **App files** in the app's private storage:
  - settings (receiver name, language, picture and audio options);
  - a random receiver ID, created on first launch, that your devices use to recognise the TV;
  - the receiver's own AirPlay pairing key, generated on the TV;
  - a flag that remembers whether Ayna Pro is active, so a paying user never sees ads at
    start-up.

  These files are removed when you clear the app's data or uninstall it. Android backup is turned
  off for Ayna, so they are not copied to your Google account.

## 2. What other devices on your network can see

To be found, Ayna announces the **receiver name** and the **random receiver ID** on your local
network: Bonjour/mDNS for AirPlay and SSDP/UPnP for DLNA. For Miracast it uses Wi-Fi Direct.
Anyone on the same network can see the receiver name, so don't put personal information in it.
You can require an on-screen PIN for AirPlay connections in Settings.

## 3. When the app uses the internet

Mirroring doesn't need an internet connection. Ayna goes online only for:

- Google Play Billing (buying or restoring Ayna Pro);
- Google AdMob ads (free version only);
- media links that a DLNA app asks the TV to play. The TV then fetches that media directly from
  the address supplied by the sending app.

## 4. Third-party services

### Google Play Billing

Payments for Ayna Pro are processed by Google Play under Google's terms and the
[Google Privacy Policy](https://policies.google.com/privacy). Ayna only receives the purchase
status needed to unlock Pro; it never sees your payment details.

### Google AdMob (free version only)

The free version uses the Google Mobile Ads SDK to show full-screen ads **between** mirroring
sessions, never over your content. Google may collect and share:

- the **advertising ID** (and the app set ID);
- the **IP address**, from which an approximate location can be inferred;
- **device and app information** (model, Android version, language, app version);
- **ad interactions** and **diagnostics**.

Google uses this to serve, measure and limit ads, to prevent fraud and, where permitted, to
personalise ads. This data goes to Google, not to us. See
[How Google uses information from sites or apps that use our services](https://policies.google.com/technologies/partner-sites).

- **Your choices:** reset or delete your advertising ID, or opt out of personalised ads, in your
  device settings (on most devices *Settings → Privacy → Ads*).
- **EEA, UK and Switzerland:** personalised ads are shown only with your consent, which is asked
  through Google's consent message.
- **Ayna Pro** shows no ads, and the app does not request ads for Pro users.

## 5. Permissions

| Permission | Why Ayna needs it |
|---|---|
| `INTERNET` | Accept mirroring and DLNA connections on your local network; Play Billing; ads in the free version |
| `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE` | Read the TV's local IP address and Wi-Fi status so your devices can connect |
| `CHANGE_NETWORK_STATE`, `CHANGE_WIFI_STATE` | Set up the Wi-Fi Direct connection that Miracast uses |
| `CHANGE_WIFI_MULTICAST_STATE` | Receive discovery messages (Bonjour/mDNS for AirPlay, SSDP for DLNA) |
| `ACCESS_FINE_LOCATION`, `ACCESS_COARSE_LOCATION` | Android 12 and earlier only: Android requires location permission to discover Wi-Fi Direct (Miracast) devices. **Ayna never reads your location.** |
| `NEARBY_WIFI_DEVICES` | Android 13 and later: find nearby Wi-Fi Direct (Miracast) devices |
| `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CONNECTED_DEVICE` | Keep the receiver running in the background (foreground service type: connected device) |
| `WAKE_LOCK` | Keep the TV from sleeping during a session |
| `RECEIVE_BOOT_COMPLETED` | Start the receiver when the TV turns on |
| `SYSTEM_ALERT_WINDOW` | Bring Ayna to the front when a device connects (auto-open) |
| `com.google.android.gms.permission.AD_ID`, `ACCESS_ADSERVICES_*` *(added by the AdMob SDK)* | Use the advertising ID for ads in the free version |
| `com.android.vending.BILLING` *(added by Play Billing)* | Make purchases through Google Play |

## 6. Children

Ayna is a general-audience utility and is not directed at children under 13 (or the minimum age
in your country). We do not knowingly collect personal information from children.

## 7. Your rights

We don't collect or store your personal data on any server, so we have no personal data of yours
to access, correct, delete or transfer. Everything Ayna stores is on your TV and is removed when
you clear the app's data or uninstall it. For data processed by Google (ads, purchases), use your
device's ad settings or your [Google Account](https://myaccount.google.com), or contact Google.
You can also email krmndigital@gmail.com with any privacy question. This applies to rights under the
GDPR, UK GDPR and Turkey's KVKK (Law No. 6698) alike.

## 8. Security

AirPlay sessions use the pairing and stream encryption defined by the protocol. DLNA media travels
unencrypted on your local network, as with any DLNA device. Use Ayna on networks you trust and
enable the PIN if others share your network.

## 9. Open source

Ayna is open source (GNU GPL v3 or later). You can check every statement in this policy against
the source code: https://github.com/krmndigital-lab/yansit-tv

## 10. Changes

If this policy changes, we'll update this page and the effective date above. Significant changes
will also be mentioned in the app's release notes.

## 11. Contact

KRMN Digital · krmndigital@gmail.com

---

<a id="turkce"></a>

# Ayna Gizlilik Politikası

**Yürürlük tarihi:** 3 Kasım 2026  
**Uygulama:** Ayna (`com.yansit.tv`), Android TV ve Google TV için  
**Geliştirici:** KRMN Digital · **İletişim:** krmndigital@gmail.com

## Özet

- Ayna'da **hesap, kayıt ve sunucu yoktur**. Geliştirici olarak uygulamadan hiçbir veri almayız.
- Ekran yansıtma ve medya oynatma, kendi cihazlarınız arasında doğrudan **yerel ağınızda**
  gerçekleşir.
- **Ücretsiz sürüm Google AdMob reklamları gösterir.** Google, bu reklamları sunmak için
  cihazınızın reklam kimliğini ve diğer cihaz bilgilerini toplayabilir. **Ayna Pro'da reklam
  yoktur.**
- Satın alımları **Google Play Faturalandırma** işler.

## 1. Uygulamanın TV'nizde işlediği bilgiler

Aşağıdakilerin tümü TV'nizde kalır:

- **Gönderdiğiniz içerik**: ekran görüntüsü ve sesi, müzik ve bilgileri (parça adı, sanatçı, albüm
  kapağı) ve DLNA medyası. Yalnızca canlı oynatma için bellekte işlenir; kaydedilmez, saklanmaz,
  yüklenmez.
- **Bağlanan cihazın adı**: oturum sırasında ekranda gösterilir.
- Uygulamanın özel depolama alanındaki **uygulama dosyaları**:
  - ayarlar (alıcı adı, dil, görüntü ve ses seçenekleri);
  - ilk açılışta oluşturulan ve cihazlarınızın TV'yi tanımak için kullandığı rastgele bir alıcı
    kimliği;
  - TV'de üretilen, alıcıya ait AirPlay eşleştirme anahtarı;
  - Ayna Pro'nun etkin olup olmadığını hatırlayan bir işaret (ücretli kullanıcıya açılışta
    reklam gösterilmemesi için).

  Bu dosyalar, uygulama verilerini temizlediğinizde veya uygulamayı kaldırdığınızda silinir.
  Ayna için Android yedeklemesi kapalıdır; bu dosyalar Google hesabınıza kopyalanmaz.

## 2. Ağınızdaki diğer cihazların görebildikleri

Bulunabilmek için Ayna, **alıcı adını** ve **rastgele alıcı kimliğini** yerel ağınızda duyurur:
AirPlay için Bonjour/mDNS, DLNA için SSDP/UPnP. Miracast için Wi-Fi Direct kullanılır. Aynı ağdaki
herkes alıcı adını görebilir; bu yüzden ada kişisel bilgi yazmayın. Ayarlar'dan AirPlay
bağlantıları için ekranda gösterilen bir PIN'i zorunlu tutabilirsiniz.

## 3. Uygulamanın interneti kullandığı durumlar

Yansıtma için internet bağlantısı gerekmez. Ayna yalnızca şunlar için internete çıkar:

- Google Play Faturalandırma (Ayna Pro satın alma veya geri yükleme);
- Google AdMob reklamları (yalnızca ücretsiz sürüm);
- bir DLNA uygulamasının TV'de oynatılmasını istediği medya bağlantıları. TV bu medyayı, gönderen
  uygulamanın verdiği adresten doğrudan indirir.

## 4. Üçüncü taraf hizmetler

### Google Play Faturalandırma

Ayna Pro ödemeleri, Google'ın koşulları ve [Google Gizlilik Politikası](https://policies.google.com/privacy)
kapsamında Google Play tarafından işlenir. Ayna yalnızca Pro'yu açmak için gereken satın alma
durumunu alır; ödeme bilgilerinizi hiçbir zaman görmez.

### Google AdMob (yalnızca ücretsiz sürüm)

Ücretsiz sürüm, yansıtma oturumlarının **arasında** tam ekran reklam göstermek için Google Mobile
Ads SDK'sını kullanır; reklamlar içeriğinizin üzerinde asla gösterilmez. Google şunları toplayabilir
ve paylaşabilir:

- **reklam kimliği** (ve uygulama grubu kimliği);
- yaklaşık konumun çıkarılabildiği **IP adresi**;
- **cihaz ve uygulama bilgileri** (model, Android sürümü, dil, uygulama sürümü);
- **reklam etkileşimleri** ve **tanılama verileri**.

Google bu verileri reklam sunmak, ölçmek ve sınırlamak, dolandırıcılığı önlemek ve izin verilen
yerlerde reklamları kişiselleştirmek için kullanır. Bu veriler bize değil, Google'a gider.
Ayrıntılar: [Google, hizmetlerini kullanan site ve uygulamalardan gelen bilgileri nasıl kullanır](https://policies.google.com/technologies/partner-sites?hl=tr).

- **Seçenekleriniz:** reklam kimliğinizi cihaz ayarlarından sıfırlayabilir veya silebilir ya da
  kişiselleştirilmiş reklamları kapatabilirsiniz (çoğu cihazda *Ayarlar → Gizlilik → Reklamlar*).
- **AEA, Birleşik Krallık ve İsviçre:** kişiselleştirilmiş reklamlar yalnızca Google'ın izin
  mesajıyla alınan onayınızla gösterilir.
- **Ayna Pro** reklam göstermez; uygulama, Pro kullanıcıları için reklam istemez.

## 5. İzinler

| İzin | Ayna neden kullanır |
|---|---|
| `INTERNET` | Yerel ağınızdaki yansıtma ve DLNA bağlantılarını kabul etmek; Play Faturalandırma; ücretsiz sürümde reklamlar |
| `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE` | Cihazlarınızın bağlanabilmesi için TV'nin yerel IP adresini ve Wi-Fi durumunu okumak |
| `CHANGE_NETWORK_STATE`, `CHANGE_WIFI_STATE` | Miracast'in kullandığı Wi-Fi Direct bağlantısını kurmak |
| `CHANGE_WIFI_MULTICAST_STATE` | Keşif mesajlarını almak (AirPlay için Bonjour/mDNS, DLNA için SSDP) |
| `ACCESS_FINE_LOCATION`, `ACCESS_COARSE_LOCATION` | Yalnızca Android 12 ve öncesi: Android, Wi-Fi Direct (Miracast) cihazlarını bulmak için konum izni ister. **Ayna konumunuzu asla okumaz.** |
| `NEARBY_WIFI_DEVICES` | Android 13 ve sonrası: yakındaki Wi-Fi Direct (Miracast) cihazlarını bulmak |
| `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CONNECTED_DEVICE` | Alıcıyı arka planda çalışır tutmak (ön plan hizmeti türü: bağlı cihaz) |
| `WAKE_LOCK` | Oturum sırasında TV'nin uykuya geçmesini önlemek |
| `RECEIVE_BOOT_COMPLETED` | TV açıldığında alıcıyı başlatmak |
| `SYSTEM_ALERT_WINDOW` | Bir cihaz bağlandığında Ayna'yı öne getirmek (otomatik açılma) |
| `com.google.android.gms.permission.AD_ID`, `ACCESS_ADSERVICES_*` *(AdMob SDK'sı ekler)* | Ücretsiz sürümde reklamlar için reklam kimliğini kullanmak |
| `com.android.vending.BILLING` *(Play Faturalandırma ekler)* | Google Play üzerinden satın alma yapmak |

## 6. Çocuklar

Ayna genel kitleye yönelik bir araçtır ve 13 yaşın (veya ülkenizdeki asgari yaşın) altındaki
çocuklara yönelik değildir. Çocuklardan bilerek kişisel bilgi toplamayız.

## 7. Haklarınız

Kişisel verilerinizi hiçbir sunucuda toplamadığımız veya saklamadığımız için erişebileceğimiz,
düzeltebileceğimiz, silebileceğimiz ya da aktarabileceğimiz bir kişisel veriniz yoktur. Ayna'yın
sakladığı her şey TV'nizdedir ve uygulama verilerini temizleyerek veya uygulamayı kaldırarak silinir.
Google'ın işlediği veriler (reklamlar, satın alımlar) için cihazınızın reklam ayarlarını ya da
[Google Hesabınızı](https://myaccount.google.com) kullanın veya Google ile iletişime geçin. Her türlü
gizlilik sorunuz için krmndigital@gmail.com adresine yazabilirsiniz. Bu, 6698 sayılı Kişisel Verilerin
Korunması Kanunu (KVKK), GDPR ve UK GDPR kapsamındaki haklar için de geçerlidir.

## 8. Güvenlik

AirPlay oturumları, protokolün tanımladığı eşleştirme ve akış şifrelemesini kullanır. DLNA medyası,
her DLNA cihazında olduğu gibi yerel ağınızda şifrelenmeden iletilir. Ayna'yı güvendiğiniz
ağlarda kullanın; ağınızı başkalarıyla paylaşıyorsanız PIN'i açın.

## 9. Açık kaynak

Ayna açık kaynaklıdır (GNU GPL v3 veya sonraki bir sürümü). Bu politikadaki her ifadeyi kaynak
kodla doğrulayabilirsiniz: https://github.com/krmndigital-lab/yansit-tv

## 10. Değişiklikler

Bu politika değişirse bu sayfayı ve yukarıdaki yürürlük tarihini güncelleriz. Önemli değişiklikleri
uygulamanın sürüm notlarında da belirtiriz.

## 11. İletişim

KRMN Digital · krmndigital@gmail.com

---

## Appendix for the developer: Play Console "Data safety" answers (suggested)

The app itself sends nothing to the developer. The only collection comes from the **Google Mobile
Ads SDK in the free version**. Check these answers against Google's current
[AdMob data-disclosure guide](https://developers.google.com/admob/android/privacy/play-data-disclosure)
before submitting, and keep them in line with the SDK versions actually bundled.

| Question / data type | Suggested answer |
|---|---|
| Does the app collect or share required user data types? | **Yes**, through the Google Mobile Ads SDK (free version) |
| Encrypted in transit? | Yes (the SDK uses HTTPS) |
| Way to request deletion? | The developer holds no user data. Point to section 7; the advertising ID is reset or deleted in device settings |
| Location → Approximate location | Collected, shared (inferred from IP by the SDK): Advertising/marketing, Analytics, Fraud prevention/security |
| Device or other IDs | Collected, shared (advertising ID, app set ID): Advertising/marketing, Analytics, Fraud prevention/security |
| App activity → App interactions | Collected, shared (ad impressions/taps): Advertising/marketing, Analytics |
| App info and performance → Diagnostics | Collected (SDK diagnostics): Analytics, Fraud prevention/security |
| Required or optional? | Required in the free version; nothing in Pro (no ads requested) |
| Financial info / Purchase history | Not declared. Google Play handles purchases; the app keeps only a local "Pro active" flag |
| Precise location, personal info, photos/videos, audio, files, contacts, messages | **Not collected.** Mirrored content is processed in memory on the TV and never sent off the device by the app. Location permissions exist only because Android requires them for Wi-Fi Direct; no location API is called. |

The app does not request `ACCESS_BACKGROUND_LOCATION`. Location permissions are declared only up to
Android 12 (`maxSdkVersion="32"`), where Android requires them for Wi-Fi Direct (Miracast)
discovery; Android 13+ uses `NEARBY_WIFI_DEVICES` with `neverForLocation`.