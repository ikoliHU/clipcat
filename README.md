# clipcat

## Kiadás és frissítések

A ClipCat magától keres frissítést (indulás után, majd 6 óránként) a
`https://github.com/ikoliHU/clipcat/releases/latest/download/latest.json` alapján, és
kérésre telepíti: Windowson az NSIS-telepítővel, Linuxon az AppImage cseréjével, illetve a
deb/rpm csomagot `pkexec`-kel telepítve.

Új verzió kiadása:

1. Emeld a verziót a `src-tauri/Cargo.toml`-ban (a `tauri.conf.json` ezt használja).
2. Actions → **release** → *Run workflow*, `publish` bepipálva, opcionálisan release notes-szal.
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
