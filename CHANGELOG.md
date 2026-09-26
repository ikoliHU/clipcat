# Változásnapló

## 0.6.0 – 2026-09-26

### Updates

- Use the shared `catninth-updater` Rust library for stable GitHub release checks, patch notes, signed downloads, and installation.
- Preserve automatic checks every six hours, localized notifications, and user-triggered installation.
- Recheck recording and clip-saving activity after downloading and reserve installation under the capture operation lock.
- Stop the capture engine and release the single-instance lock before handing off to the Windows installer or restarting after installation.
- Add updater state, progress, version precedence, and capture/installation regression tests.

## 0.5.0 – 2026-09-26

### Újdonságok

- Magyar és English US fordítás, a Windows megjelenítési nyelvét követő kezdeti választással.
- Infó-szekció a beállítások alján: frissítéskeresés, licenchivatkozás és GitHub ikon.

### Javítva

- Lemezes mentés határideje, szegmensvédelem, munkamenet-azonosítás, tárhely- és RAM-korlátok.
- Rögzítőmotor és mikrofon életciklusának javítása; hardveres kódolóhibánál működő fallback.
- Beállításvalidálás és sorosított mentés; aktív felvétel védelme átállítás és frissítés közben.
- Fragmentált MP4 kézi felvételnél; lezárási hibák jelzése és részleges fájlok megőrzése.
- Szűkített IPC/CSP/fájlhozzáférés, ellenőrzött OBS-csomagolás és GLib biztonsági backport.
- Gyorsbillentyű-rögzítés megszakítása, lapozható galéria, korlátozott előnézet-cache és naplózás.
- A CI és a kiadási workflow a natív tesztek előtt elkészíti a beágyazott felületet.

### Ellenőrzés

- Regressziós tesztek és audit-bizonyítás: `docs/audit-verification.md`.

## 0.4.1 – 2026-09-21

### Javítva

- A gyorsbillentyűk mostantól olyan játékok fókuszában is működnek, amelyek elnyelik a Windows globális gyorsbillentyű-eseményeit, például a League of Legends keret nélküli módjában.
- A natív és a tartalék billentyűfigyelés közötti duplikált műveletek megelőzése.
