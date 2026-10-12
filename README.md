<p align="center">
  <img src="assets/icon.png" width="96" alt="GameSuspend">
</p>

<h1 align="center">GameSuspend</h1>

<p align="center">
  Uspí hru v popředí jednou klávesou a stejnou klávesou ji zase probudí.<br>
  Hra přestane brát CPU i GPU, ale zůstane v paměti přesně tam, kde jsi ji opustil.
</p>

<p align="center">
  <a href="https://github.com/Gargauth/GameSuspend/releases/latest"><b>⬇ Stáhnout GameSuspend.exe</b></a>
</p>

---

## Použití

1. Stáhni `GameSuspend.exe` z [Releases](https://github.com/Gargauth/GameSuspend/releases/latest) a dej ho do vlastní složky
   (vedle něj se vytvoří `settings.ini` a `gamesuspend.log`). Instalace není potřeba.
2. Spusť ho. V oznamovací oblasti se objeví modrá ikona.
3. Ve hře stiskni **Pause**: okno se minimalizuje a proces hry se uspí. Ikona se změní na měsíc.
4. Znovu **Pause**: hra se probudí a okno se vrátí do původního stavu.

| Zkratka | Akce |
| --- | --- |
| `Pause` | uspat hru v popředí / probudit uspanou hru |
| `Ctrl+Alt+Pause` | nouzové probuzení posledního uspaného procesu |

Pravým tlačítkem na ikoně se otevře menu se stavem, probuzením, logem a nastavením.
Dvojklik na ikonu probudí uspanou hru. Při ukončení aplikace, odhlášení i vypnutí
počítače se uspaná hra vždy nejdřív probudí.

Pro automatické spouštění dej zástupce na `GameSuspend.exe` do složky `shell:startup`.

## Nastavení

`settings.ini` vedle exe se vytvoří při prvním spuštění. Po úpravě stačí v menu
zvolit **Znovu načíst nastavení**.

| Sekce | Klíč | Výchozí | Význam |
| --- | --- | --- | --- |
| `Hotkeys` | `ToggleKey` | `Pause` | uspat / probudit |
| `Hotkeys` | `EmergencyKey` | `^!Pause` | nouzové probuzení |
| `Timing` | `MinimizeDelayMs` | `350` | pauza mezi minimalizací a uspáním (0–10000) |
| `Timing` | `ResumeDelayMs` | `250` | pauza mezi probuzením a obnovením okna (0–10000) |
| `Behavior` | `MinimizeWindow` | `1` | minimalizovat okno před uspáním |
| `Behavior` | `ShowTrayTips` | `1` | zobrazovat oznámení |
| `Blacklist` | `Extra` | | další procesy, které se nesmí uspat, oddělené čárkou |

Zkratky používají syntaxi AutoHotkey: `^` Ctrl, `!` Alt, `+` Shift, `#` Win a název
klávesy (`Pause`, `ScrollLock`, `F1`–`F24`, `Insert`, `Home`, `PgUp`, `A`–`Z`, `0`–`9`,
`Numpad0`–`Numpad9`, …) nebo kód virtuální klávesy (`vk13`). Kombinace dvou kláves
přes `&` a rozlišení levého/pravého modifikátoru podporované nejsou.

## Jak to funguje

GameSuspend zavolá `NtSuspendProcess` / `NtResumeProcess` z `ntdll` na proces okna
v popředí. Pořadí je důležité:

- **uspání:** minimalizovat okno → počkat → uspat proces
- **probuzení:** probudit proces → počkat → obnovit okno

Uspaný proces neodpovídá na zprávy okna, takže se okno minimalizuje ještě za běhu hry
a obnoví až po jejím probuzení.

Systémové procesy (`explorer.exe`, `dwm.exe`, `csrss.exe`, `winlogon.exe`, …),
AutoHotkey a GameSuspend samotný se uspat nedají nikdy, bez ohledu na nastavení.

## Řešení problémů

- **„přístup odepřen“** – hra běží jako správce nebo ji chrání anticheat. Zkus spustit
  GameSuspend jako správce. Hry s kernelovým anticheatem uspat nepůjde.
- **Online hry** se během uspání obvykle odpojí od serveru.
- **Zkratka nefunguje** – stejnou zkratku už používá jiná aplikace; GameSuspend to
  ohlásí při startu. Změň ji v `settings.ini`.
- **Hra zůstala zamrzlá** – stiskni nouzové probuzení. Co se stalo, je v `gamesuspend.log`.

## Sestavení ze zdrojů

```sh
cargo build --release   # target/release/gamesuspend.exe
cargo test
```

Ikony jsou v `assets/`, generuje je `python assets/make_icons.py` (Pillow).

Release vznikne automaticky po pushnutí tagu shodného s verzí v `Cargo.toml`:

```sh
git tag v1.0.0
git push origin v1.0.0
```

## Licence

[MIT](LICENSE)
