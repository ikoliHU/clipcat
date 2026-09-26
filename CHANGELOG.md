# Változásnapló

## Kiadatlan

- Magyar és English US fordítás, a Windows megjelenítési nyelvét követő kezdeti választással.
- Infó-szekció a beállítások alján: frissítéskeresés, előkészített licenchivatkozás és GitHub ikon.
- Lemezes mentés határideje, szegmensvédelem, munkamenet-azonosítás, tárhely- és RAM-korlátok.
- Rögzítőmotor és mikrofon életciklusának javítása; hardveres kódolóhibánál működő fallback.
- Beállításvalidálás és sorosított mentés; aktív felvétel védelme átállítás és frissítés közben.
- Fragmentált MP4 kézi felvételnél; lezárási hibák jelzése és részleges fájlok megőrzése.
- Szűkített IPC/CSP/fájlhozzáférés, ellenőrzött OBS-csomagolás és GLib biztonsági backport.
- Gyorsbillentyű-rögzítés megszakítása, lapozható galéria, korlátozott előnézet-cache és naplózás.
- Regressziós tesztek és audit-bizonyítás: `docs/audit-verification.md`.

## 0.4.1 – 2026-09-21

### Javítva

- A gyorsbillentyűk mostantól olyan játékok fókuszában is működnek, amelyek elnyelik a Windows globális gyorsbillentyű-eseményeit, például a League of Legends keret nélküli módjában.
- A natív és a tartalék billentyűfigyelés közötti duplikált műveletek megelőzése.
