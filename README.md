# clipcat

## Nyelv és Infó

A Beállítások **Nyelv** mezője Magyar és English US között vált. Első induláskor a Windows
megjelenítési nyelve dönt; magyar Windows esetén magyar, minden más esetben English US lesz.
A mentett kézi választás később megmarad. Mentés után a felület, tálcamenü és értesítések
újraindítás nélkül váltanak nyelvet.

A beállítások alján az **Infó** rész tartalmazza a verziót, a frissítéskeresést, a licenchivatkozást
és a GitHub ikont. A repository a [catninth/clipcat](https://github.com/catninth/clipcat).
A Licenc gomb a megadott [MPL-2.0 licencfájlt](https://github.com/catninth/cutcat/blob/main/LICENSE) nyitja meg.

## Kiadás és frissítések

A ClipCat magától keres frissítést (indulás után, majd 6 óránként) a
`https://github.com/catninth/clipcat/releases/latest/download/latest.json` alapján, és
kérésre telepíti: Windowson az NSIS-telepítővel, Linuxon az AppImage cseréjével, illetve a
deb/rpm csomagot `pkexec`-kel telepítve.

Új verzió kiadása:

1. Emeld a verziót a `src-tauri/Cargo.toml`-ban és a `src-tauri/Cargo.lock` saját `clipcat`
   bejegyzésében (a `tauri.conf.json` a Cargo-verziót használja), majd frissítsd a `CHANGELOG.md`-t.
2. Commit és push után Actions → **release** → *Run workflow*, `publish` bepipálva,
   az előző kiadások formáját követő angol release notes-szal.
   Ez `v<verzió>` kiadást készít a telepítőkkel, az aláírásokkal és a `latest.json`-nal.

A `release` workflow minden éjjel `nightly` pre-release-t is készít; ezt a frissítő nem látja.

A csomagokat a frissítő minisign-kulccsal ellenőrzi. A publikus kulcs a `tauri.conf.json`-ban
van, a titkos kulcs és jelszava a repó `TAURI_SIGNING_PRIVATE_KEY` és
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secretjében. Ha a titkos kulcs elveszik, a már telepített
példányok nem tudnak többé frissíteni.

## Fejlesztés

A felület React + Tailwind CSS (Vite), forrása a `ui/` mappában; a fordítások a
`ui/locales/`-ban vannak, ezeket a Rust oldal is beolvassa.

```sh
npm install
npx tauri dev    # Vite dev szerver + az alkalmazás
npx tauri build  # telepítőcsomag
```

## Ellenőrzés

```sh
npm ci
npm test
npm run build
# Windows: a natív tesztekhez is elő kell állítani a konfigurált erőforrásokat.
powershell -NoProfile -ExecutionPolicy Bypass -File bundle-obs.ps1
cargo test --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=1
powershell -NoProfile -ExecutionPolicy Bypass -File tests/bundle.test.ps1
node tests/recording-crash.mjs
```

`npm run preview:ui` külön böngészős tesztfelületet indít a `127.0.0.1:5174` címen.
A valódi React komponenseket használja szimulált natív válaszokkal; nem indít rögzítést,
telepítőt, és nem írja át az alkalmazás mentett beállításait.

Az eredeti audit hibáinak reprodukciója, a javítások és a tesztek korlátai:
[audit-verification.md](docs/audit-verification.md).
