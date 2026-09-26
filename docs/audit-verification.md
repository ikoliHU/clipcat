# ClipCat audit – javítások és bizonyítás

Dátum: 2026-09-26. Kiindulás: `f0887bc4c0fffb19e3368c8a7a2661c8eb2f9034`, `main`.
A munka előtt a remote lehúzása megtörtént. Az origin a felhasználó pontosítása alapján
`https://github.com/catninth/clipcat`; új branch nem készült.

## Vizsgált anyag

A csatolt audit a `Beillesztett szöveg.txt` fájlban érkezett, Markdown-formátumú tartalommal,
18 sorszámozott megállapítással. Külön csatolt `.md` fájl nem volt elérhető.
Az audit állításait vizsgálati adatként kezeltük, nem végrehajtandó utasításként.

## Reprodukált hibák

`python tests/reproduce-baseline.py` az eredeti commit teljes `disk.rs` modulját, valamint
a változatlan `migrate_legacy` és `parse_resolution` függvényét fordítja egy külön tesztprogramba.
A fájlok és az `APPDATA` ideiglenes mappába kerülnek. A folyamatkeresés/leállítás tesztdupla:
valódi OBS-folyamatot nem állít le. Az elvárt működést ellenőrző mind a hat teszt elbukott:

| Eredeti hiba | Megfigyelt eredmény |
| --- | --- |
| Mentés közben nincs takarítás | 13 szegmens maradt a legfeljebb 5 helyett. |
| Leállítás törli a mentéshez tartozó fájlt | A még használt szegmens eltűnt. |
| Sikertelen törlés nyilvántartásból kiesik | A zárolást modellező könyvtár megmaradt, de már nem szerepelt a gyűrűben. |
| Visszafelé állított óra | `attempt to subtract with overflow` pánik a vágásnál. |
| Idegen OBS migrációs leállítása | Egy idegen PID kapott leállítási hívást, a teszt-sentinel eltűnt. |
| Korlátlan felbontás | `4294967294x4294967294` elfogadva. |

A reprodukciós szkript csak akkor tér vissza sikerrel, ha mind a hat eredeti hiba előjön.
Ezért a kimenetében a hat `FAILED` az elvárt bizonyíték, nem a javított alkalmazás teszteredménye.

Az eredeti frontenddel a fókuszvesztés, a gyorsbillentyű-rögzítés határideje és az 5 Mbps érték
megőrzése is elbukott. A javítás után ezek a regressziós tesztek sikeresek.

Linux konténerben az eredeti `glib 0.18.5` string-iterátora optimalizált fordítással
`SIGSEGV` hibát adott. Az upstream két soros javításával ugyanaz a teszt sikeres,
1000 ismétléssel és mindkét irányú iterálással. Szkript: `tests/linux-glib.sh`.

A `node tests/recording-crash.mjs` szintetikus, 160×90-es videót kódol, majd csak a saját
FFmpeg-folyamatát szakítja meg. A hagyományos MP4 nem dekódolható (`moov atom not found`);
a tényleges Rust-kódból kiolvasott fragmentálási és flush beállításokkal a fájl dekódolható.
Ez a konténerformátum tulajdonságát ellenőrzi; nem szimulál minden libobs-, driver- vagy áramhibát.

## Az audit 18 pontja

| Pont | Módosítás | Ellenőrzés, bizonyítás határa |
| --- | --- | --- |
| 01 – lemezes mentés, kvóta | Csak az aktuális mentés szegmensei védettek; közben folytatódik a takarítás. Byte-kvóta, 1 GiB szabadhely-tartalék, a mentett adathoz további helyigény; FFmpeg 60 s határidővel és megszakítással. Sikertelen törlés újrapróbálható. | Eredeti kód reprodukciója; szegmens- és törlési tesztek; valódi elakadt gyermekfolyamat kilövése; helyhiány írás nélküli modellezése. Fizikailag nem töltöttünk be meghajtót. |
| 02 – idegen OBS | A folyamatnév szerinti leállítás és az OBS-sentineltörlés kikerült. Csak ClipCat saját korábbi metaadatai takaríthatók. | Eredeti függvény tesztduplával bizonyított; a javítás tesztje megőrzi az idegen sentinelt és a videót. |
| 03 – RAM | A kódolt puffer plafonja a teljes RAM 1/8-a, a pillanatnyilag elérhető RAM 1/4-e és 2 GiB közül a legkisebb. Tartalék mellett rövidülhet a tényleges pufferidő; túl kevés memória esetén nem indul. A felület jelzi a keretet/időt. | Kis és nagy RAM-készlet tesztje; mentés előtt a puffer felső mérete is beleszámít a szükséges tárhelybe. Ez nem a teljes folyamat RAM-korlátja. |
| 04 – kódoló-fallback | Regisztrált encoder/source azonosítók ellenőrzése; sikertelen tényleges kimenetindításkor következő kódoló kipróbálása. Az aktív kódoló látható; x264 esetén CPU-terhelési jelzés. | Libobs API-tesztdupla: hiányzó azonosító, meghiúsuló NVENC-indítás, sikeres x264. Valódi GPU-kon további ellenőrzés szükséges. |
| 05 – mentési versenyek | Saját session könyvtár/azonosító, mentési guard és szálbevárás. Régi callback nem módosít új sessiont. Átállítás aktív mentés/felvétel alatt tiltott; replay-helyreállítás megtartja a kézi felvételt. | Régi fájltörlés reprodukciója; két session, régi callback, mentési szál és aktív kézi kimenet tesztjei. |
| 06 – mikrofonmutató | A FFI-használatot és felszabadítást ugyanaz a mutex védi. | Két szálas teszt: a felszabadítás a kölcsönzés elengedéséig vár. |
| 07 – részleges inicializálás | `Engine::Drop` már a libobs sikeres indulásától kezeli a részlegesen felépült objektumokat. | 100 hibás pipeline-építési ciklus: minden létrejött encoder és a libobs felszabadul a tesztduplában. |
| 08 – beállítások | Betöltési normalizálás, páros és korlátozott felbontás, hibás/NUL-os értékek javítása vagy elutasítása. Előbb motoralkalmazás, utána fájlmentés; hibánál visszaállítás. Sorosított írás és műveletek. | Extrém felbontás régi reprodukciója; hibás JSON/értékek; 16 szálon 160 fájlmentés; aktív felvétel átállításának elutasítása. A valós driver visszaállási hibája külön futtatást igényel. |
| 09 – tétlen hangforrás | Új beállításnál a mikrofon kikapcsolt. Hangforrás csak tényleges rögzítési szándéknál él; kikapcsoláskor/tétlen állapotban felszabadul. Sikertelen indulás visszavonja a szándékot. | Tétlen/off és hibás indulási tesztek. WASAPI-eszköznyitást ezen a gépen nem mértünk. |
| 10 – MP4, lezárás | Kézi felvétel fragmentált MP4-be, rendszeres flush mellett. Lezárási timeout/hibakód/üres fájl nem eredményez sikeres mentési eseményt; részleges fájl megmarad. | Valódi szintetikus FFmpeg-megszakítás; üres és hibás kimenet natív tesztje. Áramkimaradásnál az utolsó töredék továbbra is elveszhet. |
| 11 – CSP, IPC, média | CSP, csak főablakhoz rendelt alkalmazásparancsok; toast csak eseményt hallgat. Videókra korlátozott, aktuális gyökérmappát minden kérésnél ellenőrző protokoll. Mappaváltás natív tallózóhoz kötött. | Konfigurációs tesztek; nem videó/idegen fájl elutasítása; korábbi gyökér hozzáférésének visszavonása; toast tiltása; korlátozott HTTP-range. Nem teljes penetrációs teszt. |
| 12 – OBS-csomag | OBS és FFmpeg rögzített SHA256; ZIP-útvonalvalidálás; dedikált célmappa ellenőrzése; fájlonként ellenőrzött cache; saját temp mappák ellenőrzött takarítása hibaágon is. | PowerShell tesztek manipulált útvonalakkal/hash-sel/cache-sel; valódi bundle és ismételt cache-ellenőrzés. A helyi manifest nem egy helyi támadó elleni aláírás. |
| 13 – GLib | Hitelesített `0.18.5` forráscsomag, upstream `PR #1343` két soros backportja, Cargo patch. | Linux release teszt: eredeti SIGSEGV, javított siker. A verziószám marad 0.18.5, ezért verzióalapú audit továbbra is jelezheti. |
| 14 – frissítőverseny | Letöltés után az aktuális motorállapot ismételt ellenőrzése ugyanazon műveleti zár alatt, amely a rögzítési indításokat védi. Telepítési jelző blokkolja az új indítást. | Letöltés közben indult felvétel elutasítja a telepítést; 100 két szálas versenyben a két művelet nem sikerülhet egyszerre. Valódi telepítőt nem futtattunk. |
| 15 – gyorsbillentyű-rögzítés | Blur, rejtett dokumentum, unmount/AbortSignal és 30 s határidő leállítja. IPC-hibánál is visszaenged; natív oldalon fókuszvesztés és 35 s őr védi. | Frontend: blur, timeout, unmount a függő IPC közben, IPC-elutasítás. |
| 16 – galéria | Az 500-as levágás megszűnt, a fájlkeresés blokkoló munkaszálon fut. A UI 60 elemenként lapoz és összméretet mutat. | 510 fájlos natív teszt, 501. klip megnyitása a React felületen. A teljes metaadatlista még memóriába kerül; nem adatbázis alapú indexelés. |
| 17 – óravisszaállítás | A lemezes megőrzés és vágási idő `Instant` alapú; a falióra csak fájlnév/UI célra kell. | Eredeti visszafelé álló órával overflow reprodukálva; az új megőrzési algoritmus monoton időpontokon tesztelve. |
| 18 – egyéb erőforrások | IndexedDB-képek csak láthatóságkor töltődnek, 32 MiB inaktív RAM-cache célkeret, URL/listener/failed takarítás. 5 Mbps minimum egységes. Korlátos naplósor, 5 MiB aktív + előző log. PTT-n kívül 200 ms mikrofonfigyelés. CI-action SHA-k, rögzített Node/Rust, írásjog csak publish jobban. | Cache-láthatóság/törlés, 5 Mbps megőrzés, 1000 naplósor és nagy régi log, workflow-engedélyezés tesztjei. A látható csempék cache-e átmenetileg túllépheti a célkeretet. |

## Lefutott ellenőrzések

- Windows: 31 natív regressziós teszt sikeres (Rust 1.97.1, GNU target).
- Frontend: 11 Vitest teszt és 3 Node konfigurációs teszt sikeres.
- PowerShell: 14 csomagolási ellenőrzés sikeres.
- Linux: optimalizált GLib regresszió sikeres, az eredeti hibát ugyanott reprodukáltuk.
- Szintetikus MP4-megszakítás: régi hibás, új dekódolható.
- TypeScript és Vite production build sikeres; `npm audit --omit=dev`: 0 ismert sérülékenység.
- Valódi OBS/FFmpeg bundle felépült és SHA256-tal ellenőrzött; a következő futás ellenőrzött cache-t használt.
- Böngészős UI-próba: Magyar/English US választás, mentés utáni azonnali fordítás, Infó és GitHub ikon.
- `git diff --check` sikeres.

A helyi GNU linker `.rsrc merge failure: multiple non-default manifests` figyelmeztetést adott;
a tesztprogramok sikeresen futottak. Aláírt telepítő és MSVC-release ebben a futásban nem készült.

## Ismétlés és korlátok

A szokásos parancsok a README-ben vannak. A régi forrás reprodukciójához `rustc` és megfelelő
linker kell a PATH-ban. Windows GNU target esetén a script `gcc` linkert választ.
Linux GLib külön, csak olvasható repository-mounttal:

```powershell
docker run --rm --name clipcat-glib-regression --mount "type=bind,source=$PWD,target=/source,readonly" rust:1.97.1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c sh /source/tests/linux-glib.sh
```

Nem történt valódi képernyő-/mikrofonrögzítés, telepítés, frissítőtelepítés, fizikai lemezbetöltés
vagy hosszú GPU/SSD terhelés. A teljes Linux Tauri GUI és a CI-workflow még külön futtatandó.
A natív motor tesztjei a saját életciklus- és szinkronizációs kódot ellenőrzik libobs-tesztduplával;
nem bizonyítják egy adott driver vagy mikrofon hibamentességét. A libobs/driver belső elakadására
nincs általános megszakítási garancia.

Összeomlás után saját, elkülönített puffer-session könyvtár maradhat a puffer mappában.
A program ismeretlen korábbi sessionöket nem töröl automatikusan; a szabadhely-védelem ezek
helyfoglalását is figyelembe veszi. Befejezett felvételeket automatikus tárhelyfelszabadítás nem töröl.

## Elsődleges források

- A Windows megjelenítési nyelvéhez: [GetUserDefaultUILanguage](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserdefaultuilanguage).
- Jogosultságokhoz: [Tauri capabilities](https://v2.tauri.app/security/capabilities/).
- Az encoder konstrukció és a tényleges inicializálás megkülönböztetéséhez: [OBS 32.2.2 obs-encoder.c](https://github.com/obsproject/obs-studio/blob/32.2.2/libobs/obs-encoder.c).
- A muxeropciók átadásához: [OBS 32.2.2 ffmpeg-mux.c](https://github.com/obsproject/obs-studio/blob/32.2.2/plugins/obs-ffmpeg/ffmpeg-mux/ffmpeg-mux.c).
- GLib: [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html), [upstream javítás](https://github.com/gtk-rs/gtk-rs-core/pull/1343), [helyi backport leírása](../src-tauri/vendor/glib/CLIPCAT-PATCH.md).
