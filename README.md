# Keles

Rust istemci WebAssembly/WebGL2 ile masaüstü tarayıcılarında çalışır; Rust oyun
sunucusu ayrı programdır. Elle yazılmış JavaScript oyun kodu yoktur.

## Derleme ve çıktılar

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.22.1 --locked
trunk build --release --locked
cargo build --release --locked --manifest-path server/Cargo.toml
```

Web üretim çıktısı: `dist/web/` (üretilen HTML, JS bağlantı kodu, WASM ve lisanslar).
Sunucu: `server/target/release/keles-server`. Masaüstü istemci paketi üretilmez.

Yerel geliştirme (ayrı terminallerde):

```sh
./server/target/release/keles-server 127.0.0.1:9001
trunk serve --locked --dist target/web-dev
```

`http://127.0.0.1:8080` adresini açın. Debug istemci yalnızca HTTP loopback
adreslerinde sayfanın ana makinesindeki `9001` portuna bağlanır. Release istemci,
localhost üzerinde bile, daima sayfanın domain ve portunu kullanır: HTTP → WS,
HTTPS → WSS. Kareler tarayıcının requestAnimationFrame döngüsüyle zamanlanır;
FPS ayarı ekran yenileme hızının üzerine çıkamaz.

## Bağlantı ayarı

Tek WebSocket yol ayarı `index.html` içindeki
`<meta name="keles-websocket-path" content="/keles">` değeridir. Kaynakta
değiştirip yeniden derleyin veya `dist/web/index.html` içindeki aynı değeri
değiştirin. Oyuncuya bağlantı ayarı gösterilmez. Sunucu adresi üretimde sabit
bir IP veya localhost içermez.

Yerel release kontrolü için aynı origin yolunu geliştirme sunucusuyla iletin:

```sh
trunk serve --release --locked --dist target/web-check --no-autoreload --proxy-backend ws://127.0.0.1:9001/keles --proxy-rewrite /keles --proxy-ws
```

## Protokol

İstemci `JOIN|kullanici_adi` gönderir. Sunucu, oyuncuların eklenmesiyle aynı
kilit altında daha az oyuncusu olan takımı seçer; eşitlikte rastgele seçer.
`WELCOME|id|x|z|y|health|team` yanıtına `boys` veya `girls` alanı eklendi.
Eski `JOIN|ad|takım` iletilerindeki takım isteği yok sayılır. Diğer iletiler,
Camel maçı ve doğrulama sınırları korunur. İstemci ve sunucu birlikte güncellenmelidir.

```sh
cargo test --locked --lib
cargo test --locked --manifest-path server/Cargo.toml
```
