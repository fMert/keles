# Keles

Rust istemci masaüstü tarayıcılarında WebAssembly, WebGL2 ve egui ile çalışır.
Rust oyun sunucusu ayrı bir yerel programdır. Elle yazılmış JavaScript oyun kodu yoktur.

## Derleme ve yerel çalıştırma

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.22.1 --locked
trunk build --release --locked
cargo build --release --locked --manifest-path server/Cargo.toml
```

Web üretim çıktısı `dist/web/`, sunucu `server/target/release/keles-server` yolundadır.
Masaüstü grafik istemcisi veya kurulum paketi üretilmez.

Ayrı terminallerde:

```sh
./server/target/release/keles-server 127.0.0.1:9001 /tmp/keles-test.tsv
trunk serve --locked --dist target/web-dev
```

`http://127.0.0.1:8080` adresini açın. İki oyunculu deneme için normal ve gizli
pencere kullanın; aynı türdeki sekmeler oturum anahtarını paylaşır. Debug istemci
HTTP loopback adreslerinde sayfanın ana makinesindeki `9001` portuna bağlanır.

Release istemci her zaman sayfanın domain ve portunu kullanır: HTTP → WS,
HTTPS → WSS. Yerel release denemesi için aynı origin yolunu iletin:

```sh
trunk serve --release --locked --dist target/web-check --no-autoreload --proxy-backend ws://127.0.0.1:9001/keles --proxy-rewrite /keles --proxy-ws
```

WebSocket yolu `index.html` içindeki
`<meta name="keles-websocket-path" content="/keles">` değeridir. Kaynakta veya
üretilmiş HTML içinde değiştirilebilir. Oyuncuya bağlantı ayarı gösterilmez.

## Hesaplar ve oyun

Hesap için yalnızca kullanıcı adı ve şifre gerekir. Kullanıcı adı 3–16 ASCII
harf, rakam, `_` veya `-` içerir; büyük/küçük harf ayrımı yapılmaz. Şifre 8–64
kontrol dışı karakterden oluşur ve `|` içerebilir. Sahibin kararı gereği bütün
iletilerde 256 bayt sınırı korunur: bazı uzun Unicode şifreler bu sınıra sığmaz;
form bunları bağlantı açmadan açıklar.

Şifreler Argon2id varsayılanlarıyla tuzlanıp özetlenir. Pahalı işlem dünya kilidi
alınmadan, ayrı bir kilitle sırayla çalışır. IP başına dakikada 10 kimlik
doğrulama denemesi ve saatte 3 kayıt sınırı vardır. Aynı hesap bir kez çevrimiçi
olabilir. Beni hatırla seçeneği oturum anahtarını tarayıcıda saklar. Çıkış yapmak
sunucudaki anahtarı da siler. Sunucu yeniden başlatılırsa yeniden giriş gerekir.

Lobiden iki oda türüne girilir:

- **Takım Ölüm Maçı:** 16 oyuncu, dengeli iki takım, 3 can, ilk 40 leş;
  sonuçtan 10 saniye sonra tur sıfırlanır.
- **Dereceli 1v1:** 1000 başlangıç puanı, 3 saniye geri sayım, ilk 10 leş veya
  5 dakika. Ölen oyuncu 2 saniye sonra otomatik doğar. Arama aralığı daha uzun
  bekleyen oyuncunun her 10 saniyesinde 50 artar: ±100 ile başlar, ±1000'de durur.
  Aynı IP'deki oyuncular eşleştirilmez; loopback yerel test için istisnadır.
  Başlamadan ayrılma iptal, başladıktan sonra ayrılma hükmen yenilgidir.

Elo katsayısı 32'dir; eşit 1000 puanlı oyuncularda sonuç +16 / −16 olur.
Beraberlikte Elo 0,5 sonuç üzerinden hesaplanır. Sahibin kararıyla beraberlik
hem galibiyet hem mağlubiyet sütununa eklenir; böylece yalnızca berabere kalan
hesaplar da ilk 10'a girer. Kazanma yüzdesi bu iki sütundan hesaplanır.
Ligler Bronz (<900), Gümüş (900), Altın (1100), Platin (1300), Elmas (1500).

WASD hareket, fare nişan, Boşluk zıplama, sol tık ateş, Tab skor tablosu,
Esc geri/duraklatma. Fare kilidi için Oyna, Devam et veya eşleşme ekranındaki
başlama düğmesine tıklanır. Bulanıklık, görünürlük ve fare kilidi kaybı oyunu
duraklatır. Hassasiyet, arayüz ölçeği, FPS/kan seçenekleri, çözünürlük, FPS sınırı
ve nişangâh ayarları tarayıcıda saklanır; özel mod depolamayı engellerse sessizce
varsayılanlar kullanılır. Varsayılan sahne sınırı 60 FPS'dir. Menüdeki harita
sürekli 20 FPS altında kalırsa statik arka plana geçilir.

## Hesap dosyası

Sunucunun ikinci argümanı hesap dosyasıdır; verilmezse çalışma dizininde
`keles-accounts.tsv` kullanılır. Eksik dosya boş hesap kümesi demektir. Hatalı
başlık, bozuk satır, geçersiz özet veya yinelenen anahtar başlangıcı durdurur;
hesaplar sessizce atılmaz.

```text
# keles accounts v1
anahtar<TAB>GörünenAd<TAB>argon2_phc_özeti<TAB>puan<TAB>galibiyet<TAB>mağlubiyet
```

Kayıt ve her dereceli sonuç, aynı dizindeki `.tmp` dosyasına yazılıp eşitlenir,
sonra atomik olarak asıl dosyanın yerine geçirilir. Unix'te yeni dosya izinleri
0600'dür. Sonuç yazılamazsa puanlar değiştirilmez; sonuç 5 saniye arayla yeniden
kaydedilir. Bekleyen sonuç tamamlanana kadar hesap yeni bir dereceli maça giremez.
Oturumlar yalnızca bellektedir.

Çalıştırırken **kalıcı bir dosya yolu verin, üst dizini önceden oluşturun ve
dosyayı yedekleyin**. Sunucunun bu dizine yazabilmesi gerekir. Gerçek kullanımda
sayfayı HTTPS ve bağlantıyı WSS üzerinden sunun; şifreler aktarımda korunmalıdır.
Bu çalışma yalnızca yerelde yapılır; dağıtım veya uzak sunucu işlemi içermez.

## Protokol

UTF-8 metin, `|` ayraçlı. İlk ileti bağlantıdan itibaren 5 saniye içinde gelir.
Sunucu sınırları: 256 bayt ileti/çerçeve, 160 bağlantı, IP başına 4 doğrulanmış
oyuncu; 16 oyuncu sınırı yalnızca ölüm maçı odasına aittir. 5 saniyede ping,
15 saniye sessizlikte bağlantı kapatma korunur.

| Aşama | İstemci → sunucu | Sunucu → istemci |
|---|---|---|
| Kimlik | `REGISTER|ad|şifre`, `LOGIN|ad|şifre`, `TOKEN|anahtar` | `AUTH_OK|ad|anahtar|puan|G|M`, `AUTH_ERR|kod` |
| Lobi | `INFO` (en çok saniyede bir) | `INFO|çevrimiçi|ölüm_maçı|sırada|puan|G|M`, ardından `TOP|ad:puan:G:M,...` |
| Ölüm maçı | `PLAY` | `WELCOME`, `SCORE`, varsa `WIN`, mevcut oyuncular için `ADD`; doluysa `ERROR|Server is full` |
| Sıra | `QUEUE`, `UNQUEUE` | `QUEUED`, `UNQUEUED` |
| Eşleşme | — | `MATCH|rakip_adı|rakip_puanı|puanım|hedef|süre|geri_sayım`, ardından `WELCOME`, `ADD`, `SCORE|0|0`, zamanı gelince `GO` |
| Maç | `POS|x|z|ayak_y|yaw`, `FIRE|yaw|pitch`, `RESPAWN` | Aşağıdaki mevcut oyun iletileri |
| Sonuç | — | `RANKED_END|sonuç|neden|puan|fark|G|M` |
| Çıkış | `LOBBY`, `LOGOUT` | `LOBBY` sonrası `INFO` ve `TOP`; `LOGOUT` anahtarı silip bağlantıyı kapatır |

`AUTH_ERR` kodları: `bad_username`, `bad_password`, `taken`, `wrong`, `online`,
`rate_limited`, `expired`, `full`. Hatalı ad ve şifre aynı `wrong` yanıtını verir.
Hatalı kimlik doğrulamasından sonra bağlantı kapanır; her deneme yeni bağlantıdır.
Şifre ayrıştırması `splitn(3, '|')` kullanır. İstemci kodları Türkçe açıklar.

Mevcut alan sıraları korunur:

```text
WELCOME|id|x|z|y|can|takım
ADD|id|ad|x|z|y|yaw|can|takım|leş|ölüm
MOVE|id|x|z|y|yaw
HP|id|can
LEAVE|id
AT|x|y|z
SCORE|erkekler|kızlar
WIN|takım|saniye
KILL|vuran_id|ölen_id
HITMARK
ERROR|neden
```

Takım kodları `boys` ve `girls` olarak kalır. İstemci eski, istatistiksiz `ADD`
iletisini de kabul eder. `RANKED_END` sonuçları `win/loss/draw`, nedenleri
`kills/time/forfeit/cancelled` değerleridir. Oyun yayınları yalnızca aynı odaya
gider; lobi ve sıra maç olaylarını almaz. İstemci ve sunucu birlikte güncellenir.

## Kontroller

```sh
cargo fmt --all
cargo fmt --all --manifest-path server/Cargo.toml
cargo clippy --lib
cargo clippy --manifest-path server/Cargo.toml
cargo test --locked --lib
cargo test --locked --manifest-path server/Cargo.toml
trunk build --release --locked
cargo build --release --locked --manifest-path server/Cargo.toml
```

Testler: hesap doğrulama/kayıt, harf duyarsız tekillik, oturum/çıkış, hız sınırı,
dosya gidiş-dönüşü, Elo, sıra/IP koruması, oda yalıtımı, geri sayım, leş sonucu,
süre/iptal/hükmen sonuç, başarısız kayıt sonrası tekrar, mevcut hareket/hasar ve
tur sıfırlama; istemcide doğrulama, lig eşikleri ve kart yerleşimi.

## Yeni kaynak ve bağımlılık

Eklenen tek doğrudan bağımlılık sunucuda **argon2 0.5.3** (`std` özelliği,
OsRng için): [RustCrypto/password-hashes](https://github.com/RustCrypto/password-hashes),
MIT veya Apache-2.0. Olgun RustCrypto projesinde yüzlerce anlamlı commit,
CI, algoritma testleri, belgeler ve fuzz altyapısı bulunur. Kullanım örneğinden
uyarlanan parça `argon2/src/lib.rs:36–58`, commit
`5b6a74e0799955b21c9d3854de1674a78a472648` içindeki PHC hash/verify çağrılarıdır;
`server/src/accounts.rs` başında kaynak yorumu vardır. Hash kodu kopyalanmaz;
kütüphanenin varsayılanları kullanılır. İstemcide mevcut `web-sys` bağımlılığının
`Storage` özelliği açılır; yeni UI çatısı veya yazı tipi eklenmez. Diğer lisanslar
`THIRD-PARTY-LICENSES.txt` içinde korunur.
