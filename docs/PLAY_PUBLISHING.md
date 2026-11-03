# Ayna — Google Play'e otomatik yayın

Yayın, Google Play Developer API üzerinden otomatik yapılır. `vX.Y.Z` etiketi gönderildiğinde
GitHub Actions şu adımları sırayla çalıştırır: testler → imzalı AAB → gizlilik kontrolü → Play
(mağaza metinleri, görseller, ürünler, sürüm) → GitHub sürümü. Aynı işlem yerelde
`./publish.sh` ile de yapılabilir.

## Otomatik yapılanlar (`tools/play/publish.py`)

| Komut | Ne yapar |
|---|---|
| `check` | Play'deki durumu gösterir: kanallar, sürümler, diller, ürünler |
| `listing` | 10 dilde başlık, kısa ve uzun açıklama; simge, öne çıkan grafik, TV banner'ı, TV ekran görüntüleri; iletişim e-postası ve web sitesi |
| `products` | `yansit_pro_yearly` (yıllık, 10 $) ve `yansit_pro_lifetime` (ömür boyu, 20 $): tüm ülkeler için yerel fiyat dönüşümü ve etkinleştirme |
| `release --track alpha` | AAB'yi yükler, 10 dilde sürüm notuyla kanala koyar |
| `testers --group x@googlegroups.com` | Kapalı test kanalına test grubunu bağlar |
| `promote --from alpha --to production` | Test sürümünü üretime taşır (`--rollout 0.2` ile kademeli) |
| `all --track T` | listing + products + release |

Sürüm numarası tek yerde tutulur: `Cargo.toml` → `version`. versionCode bundan otomatik
hesaplanır (1.0.0 → 10000). Play daha önce 2 numaralı sürümü gördüğü için hiçbir zaman 3'ün
altına inilmez.

## Bir kez yapılması gerekenler (API'nin yapamadıkları)

1. **Hizmet hesabı (robot kullanıcı).** Google Cloud konsolunda:
   1. <https://console.cloud.google.com/projectcreate> → proje adı `krmn-play` → Oluştur.
   2. <https://console.cloud.google.com/apis/library/androidpublisher.googleapis.com> →
      **Etkinleştir** ("Google Play Android Developer API").
   3. <https://console.cloud.google.com/iam-admin/serviceaccounts> → **Hizmet hesabı oluştur** →
      ad `play-publisher` → rol vermeden **Bitti**.
   4. Hesaba tıkla → **Anahtarlar** → **Anahtar ekle → Yeni anahtar → JSON**. Bir dosya iner.
   5. Play Console → **Kullanıcılar ve izinler** → **Yeni kullanıcıları davet et** → e-posta
      alanına hizmet hesabının adresini yaz (`play-publisher@krmn-play.iam.gserviceaccount.com`
      gibi). **Uygulama izinleri → Uygulama ekle → Ayna** → tüm kutuları işaretle → **Uygula →
      Kullanıcıyı davet et**. ("API erişimi" menüsü artık yok; doğru yer burası.)
   6. İnen JSON dosyasını sunucuya yükle. Sunucuda `~/.config/krmn/play-service-account.json`
      konumuna (izin 600) konur ve GitHub secret'ı `PLAY_SERVICE_ACCOUNT_JSON` olarak eklenir.
      Bu dosya hiçbir zaman depoya girmez.

2. **Play Console'daki beyan formları.** Bunlar API ile doldurulamaz. Her uygulamada bir kez
   doldurulur; yanıtlar aşağıdaki tabloda hazır.

3. **Kapalı test (yalnızca kişisel hesaplar için).** 13 Kasım 2023'ten sonra açılan kişisel
   geliştirici hesaplarında, üretime geçmeden önce **en az 12 test kullanıcısının 14 gün
   kesintisiz** kapalı testte kalması gerekir. Bu kural her yeni uygulama için tekrarlanır.
   Kuruluş (şirket) hesaplarında bu kural yoktur; kuruluş hesabı için D-U-N-S numarası gerekir.
   1. <https://groups.google.com/my-groups> → **Grup oluştur** → ad `ayna-test`, "Kimler
      katılabilir: Web'de herkes" → Oluştur.
   2. Grup adresi bildirilir, ardından `publish.py testers --group ayna-test@googlegroups.com`
      çalıştırılır.
   3. 12 kişiye iki bağlantı gönder: grubun katılım sayfası ve
      <https://play.google.com/apps/testing/com.yansit.tv> (katılım bağlantısı).
   4. 14 gün sonra: Play Console → Kontrol paneli → **Üretime erişim başvurusu** (form yanıtları
      aşağıda). Onay gelince `promote --from alpha --to production` çalıştırılır.

## Play Console yanıtları (Uygulama içeriği)

| Bölüm | Yanıt |
|---|---|
| Gizlilik politikası | `https://krmndigital-lab.github.io/yansit-tv/` |
| Uygulama erişimi | Tüm işlevler özel erişim gerektirmeden kullanılabilir. Not (İngilizce): *Testing needs a second device on the same Wi-Fi: an iPhone/iPad/Mac (Control Center → Screen Mirroring), a Windows 10/11 PC (Win + K) or an Android phone with Miracast, or any DLNA app.* |
| Reklamlar | **Evet, reklam içeriyor** |
| İçerik derecelendirmesi | Kategori: **Araç / Yardımcı program, Üretkenlik, İletişim veya Diğer**. Şiddet, cinsellik, küfür, kumar, uyuşturucu: hepsi **Hayır**. Kullanıcılar arası etkileşim / içerik paylaşımı: **Hayır**. Konum paylaşımı: **Hayır**. Dijital ürün satın alma: **Evet**. Sınırsız internet / web tarayıcı: **Hayır** |
| Hedef kitle | **18 ve üzeri** (en sorunsuzu; 13+ da olur, 13 yaş altı seçilmemeli) |
| Veri güvenliği | Veri topluyor veya paylaşıyor: **Evet** (yalnızca AdMob SDK, ücretsiz sürüm). Aktarımda şifreli: **Evet**. Silme talebi: **Hayır** (hesap yok, geliştirici veri tutmuyor). Türler: *Yaklaşık konum*, *Cihaz veya diğer kimlikler*, *Uygulama etkileşimleri* (toplanıyor ve paylaşılıyor; amaç: Reklam, Analiz, Dolandırıcılık önleme); *Teşhis* (toplanıyor; Analiz, Dolandırıcılık önleme). Hepsi "zorunlu". Ayrıntı: `docs/store/privacy-policy.md` ek bölüm |
| Reklam kimliği | **Evet**, amaç: Reklam |
| Ön plan hizmeti | Tür **Bağlı cihaz**. Açıklama: *Keeps the AirPlay/Miracast/DLNA receiver reachable so phones and PCs can connect while the TV shows other apps.* Video gerekirse: telefondan yansıtma başlatılırken TV'nin kaydı |
| Kamu, finans, sağlık, haber uygulaması | Hepsi **Hayır** |
| Kategori | **Video Oynatıcılar ve Düzenleyiciler** |
| Cihaz türleri | Sürüm → Gelişmiş ayarlar → **Cihaz türleri → Android TV** katılımı |
| Fiyat | Ücretsiz, tüm ülkeler |

## Sonraki her sürüm

`Cargo.toml` içindeki `version` artırılır, `docs/store/release-notes/<sürüm>/` altına notlar
yazılır ve `git tag vX.Y.Z && git push --tags` çalıştırılır. Gerisini GitHub Actions yapar.
Hangi kanala gideceğini GitHub değişkeni `PLAY_TRACK` belirler (`alpha` → üretim izni gelince
`production`).
