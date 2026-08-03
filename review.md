# Preisprüfung k-vet

Dieses Dokument ist für **Dr. Erika Musterfrau**. Es sammelt alle Stellen, an denen das Programm
Geldbeträge berechnet, und stellt sie als konkrete Zahlenbeispiele dar — ohne Programmcode.

Warum das nötig ist: das Programm rechnet GOT-Gebühren, Arzneimittelpreise nach der
Arzneimittelpreisverordnung (AMPreisV) und das Wegegeld selbst aus. Ob dabei die **richtigen**
Beträge herauskommen, kann nur jemand beurteilen, der diese Preise täglich abrechnet. Das sind Sie.

## Wie prüfen Sie das?

* Arbeiten Sie **einen Abschnitt pro Sitzung** durch, nicht alles auf einmal. Die Abschnitte sind
  unabhängig voneinander.
* Jeder Punkt hat eine **Nummer** (z. B. `AMP-03`). Wenn etwas unklar ist oder nicht stimmt, sagen
  Sie einfach *„Punkt AMP-03 stimmt nicht"* — dann wissen wir sofort, wo wir nachsehen müssen.
* Kreuzen Sie bei jedem Punkt an: **bestätigt** oder **Rückfrage**. Bei einer Rückfrage schreiben
  Sie bitte kurz dazu, welchen Betrag Sie erwartet hätten.
* Sie brauchen dafür: die **GOT 2022**, eine **AMPreisV**-Tabelle und ein paar Ihrer **alten
  Rechnungen** zum Gegenlesen.
* Die Zeile *Fundstelle* am Ende jedes Punktes ist nur für uns Entwickler — die können Sie
  überspringen.

**Am wichtigsten sind `GOT-01` und `MWS-03`.** Der erste betrifft einen Fehler, den wir gefunden
haben; der zweite eine Stelle, an der Ihre Rechnung künftig bewusst anders aussieht als bisher.

---

## A · Leistungen nach GOT

### GOT-01 · Nettopreise der GOT — hier haben wir einen Fehler gefunden

Die GOT nennt **Nettopreise**. Auf die Rechnung gehört der Gebührensatz **plus 19 %
Mehrwertsteuer**. Genau das hat das Programm bisher **nicht** gemacht: es hat den GOT-Betrag
behandelt, als wäre die Mehrwertsteuer schon enthalten. Dadurch wäre jede GOT-Leistung um rund
16 % zu günstig abgerechnet worden.

Wir haben es an Ihrer Rechnung **RE-289** gegengeprüft:

| GOT-Nr. | Leistung                                        | GOT netto | + 19 % MwSt | Auf RE-289  |
| ------- | ----------------------------------------------- | --------- | ----------- | ----------- |
| 16      | Allgemeine Untersuchung mit Beratung, Hund/Katze | 23,62 €   | **28,11 €** | 28,11 €     |
| 17      | Allgemeine Untersuchung, Heimsäugetiere          | 15,39 €   | **18,31 €** | 18,31 €     |
| 40      | Hausbesuch                                       | 34,50 €   | **41,06 €** | 41,05/41,06 € |
| 251     | Verband anlegen oder abnehmen                    | 17,25 €   | **20,53 €** | 20,53 €     |
| 394     | Untersuchung der Haut/Wunde                      | 16,50 €   | **19,64 €** | 19,63/19,64 € |
| 662     | Otitis externa, Behandlung, je Seite             | 10,26 €   | **12,21 €** | 12,21 €     |

**Frage:** Ist 28,11 € der Betrag, den Sie der Tierhalterin für GOT-Nr. 16 in Rechnung stellen —
und nicht 23,62 €? (Bei GOT 40 und 394 weicht Ihre alte Rechnung um einen Cent zwischen Einzelpreis
und Gesamtsumme ab; darum geht es in Punkt `MWS-03`.)

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

*Fundstelle: `migrations/0009_net_prices_and_settings.sql`, `src/domain/money.rs`*

### GOT-02 · Es ist immer der einfache Gebührensatz hinterlegt

Im Programm sind alle GOT-Positionen mit dem **einfachen Satz** gespeichert. Den Faktor (z. B. das
1,5-fache oder 2-fache) stellen Sie erst **an der einzelnen Rechnungszeile** ein.

Beispiel: GOT-Nr. 16 zum 1,5-fachen Satz → 23,62 € × 1,5 = 35,43 € netto → **42,16 € brutto**.

**Frage:** Ist das so richtig — Grundpreis einfacher Satz, Faktor pro Zeile?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

*Fundstelle: `migrations/0008_got_import.sql:1-8`*

### GOT-03 · Chirurgische Positionen sind ausgeblendet

Die chirurgischen GOT-Positionen sind zwar vorhanden, tauchen aber in der Suche und in den Listen
**nicht** auf, weil die Praxis keine Chirurgie anbietet.

**Frage:** Soll das so bleiben? Falls Sie doch gelegentlich eine chirurgische Position abrechnen,
schalten wir sie wieder ein.

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

*Fundstelle: `migrations/0008_got_import.sql:7-8`*

### GOT-04 · Stichprobe aus dem Gebührenverzeichnis

Wir haben beim Einlesen der 931 GOT-Positionen stichprobenartig geprüft. Bitte lesen Sie diese
fünf gegen Ihr GOT-Heft gegen (die Beträge sind **netto**, wie im Heft):

| GOT-Nr. | Leistung                                       | hinterlegt netto |
| ------- | ---------------------------------------------- | ---------------- |
| 1       | Beratung im einzelnen Fall ohne Untersuchung    | 11,26 €          |
| 3       | Dokumentation aufgrund gesetzlicher Vorgaben    | 11,20 €          |
| 20      | Allgemeine Untersuchung, nicht domestizierte …  | 36,94 €          |
| 411     | Chirurgische Entfernung einer Warze             | 32,99 €          |
| 1005    | Nephrotomie                                     | siehe Programm   |

Bitte prüfen Sie zusätzlich **fünf beliebige Positionen Ihrer Wahl**, am besten solche, die Sie
häufig abrechnen.

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

*Fundstelle: `migrations/0008_got_import.sql:10-16`*

### GOT-05 · Mehrwertsteuersatz auf Leistungen

Alle GOT-Positionen sind mit **19 %** hinterlegt.

**Frage:** Gibt es Leistungen, die Sie mit **7 %** abrechnen? Falls ja: welche?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

---

## B · Arzneimittel (AMPreisV)

> **Korrigiert nach Ihrer Rückmeldung.** Hier stand vorher, Ausgangspunkt sei „Ihr Einkaufspreis".
> Das war falsch formuliert, und Sie haben zu Recht widersprochen. Maßgeblich ist der
> **Listenpreis**, nicht der Betrag, den Sie nach Rabatten tatsächlich zahlen. Am gerechneten
> Ergebnis ändert sich nichts — das Programm hat immer schon mit dem Feld „Listenpreis (netto)"
> gerechnet. Nur die Erklärung war irreführend.

Das Programm rechnet den Verkaufspreis selbst aus. Ausgangspunkt ist immer der **Listenpreis ohne
Mehrwertsteuer** — also der Preis aus der Liste Ihres Großhändlers.

Wichtig dabei:

* Ein **Rabatt, den Sie ausgehandelt haben, senkt die Grundlage nicht.** § 3 Abs. 2 AMPreisV nennt
  ausdrücklich den „Abgabepreis des pharmazeutischen Unternehmers ohne die Umsatzsteuer" zuzüglich
  des Großhandelszuschlags — also einen *gelisteten* Preis. Was Sie im Einzelfall bezahlt haben,
  spielt dafür keine Rolle. Sie dürfen den Listenpreis zugrunde legen, auch wenn Sie günstiger
  eingekauft haben.
* Der **Großhandelszuschlag ist bereits enthalten**, wenn Sie den Preis aus der Liste Ihres
  Großhändlers nehmen. Sie müssen dazu nichts rechnen.
* **Vorsicht bei der Wortwahl:** Preislisten für Tierärzte — auch die Barsoi-Liste — nennen genau
  diesen Betrag „Einkaufspreis". Gemeint ist dort der *gelistete* Einkaufspreis, also dasselbe wie
  unser „Listenpreis". Wir nennen das Feld bewusst „Listenpreis", damit keine Verwechslung mit
  Ihrem tatsächlich gezahlten Preis entsteht.

> **Bereits mit Ihnen abgestimmt, hier nur zur Ablage** — kein weiterer Prüfpunkt:
> **Humanpräparate.** Ein für Menschen zugelassenes Mittel, das Sie am Tier anwenden, wird nach
> § 3 Abs. 1 Satz 2 abgerechnet: **3 % zuzüglich 8,10 €**, nicht nach der Staffel unten. Sie
> kennzeichnen das am Medikament mit dem Haken „Humanpräparat".
> **Teilmengen von Humanpräparaten.** Weil die ganze Packung die festen 8,10 € trägt, eine
> Teilmenge nach § 4 aber nur einen Prozentsatz, kann eine Teilmenge unter ihrem anteiligen
> Packungspreis liegen. Auf Ihren Wunsch gibt es dafür eine Untergrenze, die in der
> Konfigurationsdatei eingeschaltet wird.

### AMP-01 · Prozentuale Zuschläge nach § 3 Abs. 3

| Listenpreis (netto) | Zuschlag | Verkauf netto | Verkauf **brutto** (19 %) |
| ------------------- | -------- | ------------- | ------------------------- |
| 1,00 €              | 0,68 € (68 %) | 1,68 €   | **2,00 €**                |
| 10,00 €             | 4,80 € (48 %) | 14,80 €  | **17,61 €**               |
| 40,00 €             | 12,00 € (30 %) | 52,00 € | **61,88 €**               |

**Frage:** Sind das die Preise, die Sie berechnen würden?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

*Fundstelle: `src/domain/money.rs`, Abschnitt AMPreisV*

### AMP-02 · Feste Zuschlagsbeträge nach § 3 Abs. 4

Zwischen den Prozentstufen gibt die Verordnung feste Beträge vor:

| Listenpreis (netto) von–bis | fester Zuschlag |
| --------------------------- | --------------- |
| 1,23 € – 1,34 €             | 0,83 €          |
| 19,43 € – 22,57 €           | 8,35 €          |
| 29,15 € – 35,94 €           | 10,78 €         |

**Frage:** Stimmen diese drei Stufen mit Ihrer AMPreisV-Tabelle überein?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### AMP-03 · Ermäßigte Sätze bei teuren Präparaten (§ 10 Abs. 2)

Ab einem Listenpreis von **51,13 €** wird der übersteigende Teil geringer bezuschlagt: bis
127,82 € mit 25 %, darüber mit 20 %.

| Listenpreis (netto) | Zuschlag | Verkauf **brutto** (19 %) |
| ------------------- | -------- | ------------------------- |
| 100,00 €            | 27,56 €  | **151,79 €**              |
| 200,00 €            | 48,95 €  | **296,25 €**              |

Die Spanne wird also mit steigendem Listenpreis kleiner — so ist die Verordnung gedacht.

**Frage:** Passt das zu dem, was Sie bei teuren Präparaten abrechnen?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### AMP-04 · Teilmengen (§ 4) — Umfüllen kostet mehr

Wenn Sie aus einer Originalpackung eine Teilmenge abgeben, gilt ein Zuschlag von **100 %** auf den
anteiligen Listenpreis.

Beispiel: 100-ml-Flasche, Listenpreis 10,00 € netto.

| Abgabe   | anteiliger Listenpreis | Zuschlag | Verkauf **brutto** |
| -------- | ---------------------- | -------- | ------------------ |
| 10 ml    | 1,00 €                 | 1,00 €   | **2,38 €**         |
| 50 ml    | 5,00 €                 | 5,00 €   | **11,90 €**        |
| bei 7 % MwSt: 10 ml | 1,00 €      | 1,00 €   | **2,14 €**         |

Folge daraus: die **ganze Flasche als Teilmenge** abzugeben ist teurer, als die Originalpackung zu
verkaufen (dort gilt in dieser Preisklasse nur 48 %). Das ist gewollt — es ist der Aufwand fürs
Umfüllen.

**Frage:** Ist der 100-%-Zuschlag für Teilmengen richtig, und ist der Effekt gewollt?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### AMP-05 · Welchen Betrag tragen Sie als „Listenpreis (netto)" ein?

Das Programm rechnet alle Zuschläge auf diesen einen Wert. Er sollte aus der Preisliste Ihres
Großhändlers stammen und **ohne Mehrwertsteuer** angegeben sein.

**Frage:** Nehmen Sie den Betrag aus der Liste Ihres Großhändlers — und nicht den Preis, den Sie
nach Rabatt tatsächlich überwiesen haben? Und ist dieser Betrag netto?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

---

## C · Wegegeld (GOT § 10)

### WEG-01 · Sätze

Hinterlegt sind **3,50 € je Doppelkilometer** und **mindestens 13,00 €** je Fahrt — beides
**netto**, so wie die GOT es angibt. Auf der Rechnung erscheint der Bruttobetrag: 3,50 € netto
sind **4,17 € brutto**. Genau dieser Betrag steht auch auf Ihrer Rechnung RE-289 in der Zeile
„Wegegeld (anteilig) je gefahrener Doppelkilometer".

**Frage:** Sind das die aktuellen Sätze? (Sie stehen in einer Einstellungsdatei und lassen sich bei
einer GOT-Änderung leicht anpassen.)

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### WEG-02 · Sie geben die **einfache** Strecke ein

Sie tragen die Entfernung **hin** ein; dass der Rückweg mit abgerechnet wird, steckt bereits im
Begriff „Doppelkilometer".

| Eingabe | netto     | **brutto**    |
| ------- | --------- | ------------- |
| 10 km   | 35,00 €   | **41,65 €**   |
| 27 km   | 94,50 €   | **112,46 €**  |

**Frage:** Geben Sie die einfache Strecke ein — oder rechnen Sie bisher Hin- und Rückweg zusammen?
Das ist die Stelle, an der ein Missverständnis den Betrag verdoppeln würde.

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### WEG-03 · Mindestbetrag bei kurzen Strecken

| Eingabe | rechnerisch netto | berechnet netto             | **brutto**  |
| ------- | ----------------- | --------------------------- | ----------- |
| 2 km    | 7,00 €            | **13,00 €** (Mindestbetrag) | 15,47 €     |
| 4 km    | 14,00 €           | 14,00 €                     | 16,66 €     |

Der Mindestbetrag greift also bis etwa 3,7 km. (Auf RE-289 wurden 3,71 Doppelkilometer mit
15,47 € abgerechnet — das ist genau dieser Mindestbetrag.)

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### WEG-04 · Faktor 1 bis 3 bei widrigen Verhältnissen

Die GOT erlaubt bis zum **Dreifachen** des Wegegeldes. Höhere Eingaben werden automatisch auf 3
begrenzt.

**Frage:** Nutzen Sie diesen Faktor, und ist die Begrenzung auf 3 richtig?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### WEG-05 · Geteilte Fahrten

Es sind auch Nachkommastellen möglich, z. B. wenn Sie eine Fahrt auf zwei Kundinnen aufteilen:
**7,5 km → 26,25 € netto → 31,24 € brutto**.

**Frage:** Teilen Sie Fahrten so auf? Wenn ja: nach Kilometern (wie hier) oder hälftig?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

---

## D · Mehrwertsteuer und Rundung

### MWS-01 · Die Mehrwertsteuer wird **auf** den Nettobetrag gerechnet

Und zwar einmal je Steuersatz auf die Summe — nicht auf jede Zeile einzeln. Beispiel mit zwei
Zeilen zu je 0,05 € netto: 19 % auf 0,10 € = **0,02 €**. Zeile für Zeile gerechnet käme
0,01 € + 0,01 € heraus — dieselbe Zahl, aber bei größeren Rechnungen weicht es ab, und die
Steuerzusammenstellung muss zur Summe der Zeilen passen.

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### MWS-02 · Kaufmännisches Runden auf Cent

Ab einem halben Cent wird aufgerundet (0,125 € → 0,13 €).

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### MWS-03 · Der Cent in der Spalte „Gesamt" — hier ändern wir etwas

Auf Ihrer bisherigen Rechnung **RE-289** steht beim Hausbesuch (GOT 40) als Einzelpreis
**41,05 €**, als Gesamtbetrag derselben Zeile aber **41,06 €**. Bei GOT 394 genauso: **19,63 €**
gegen **19,64 €**. Das ist kein Tippfehler, sondern eine Rundungsdifferenz — sie entsteht, weil der
Bruttobetrag aus dem Nettobetrag errechnet wird.

k-vet verteilt diesen Cent künftig so, dass die **Spalte am Ende exakt aufgeht**. Beispiel: drei
Positionen zu je 3,33 € netto ergeben je 3,96 € brutto, zusammen 11,88 € — die Rechnungssumme ist
aber 11,89 €. Der fehlende Cent wird der Position zugeschlagen, die am stärksten abgerundet wurde:

| Position | netto  | brutto      |
| -------- | ------ | ----------- |
| 1        | 3,33 € | **3,97 €**  |
| 2        | 3,33 € | 3,96 €      |
| 3        | 3,33 € | 3,96 €      |
| Summe    | 9,99 € | **11,89 €** |

**Frage:** Ist Ihnen das recht? Der Vorteil: die Rechnung geht immer auf, wenn eine Kundin
nachrechnet. Der Preisunterschied beträgt nie mehr als ein Cent pro Zeile.

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### MWS-04 · Angebotene Steuersätze

Zur Auswahl stehen **19 %** und **7 %**.

**Frage:** Brauchen Sie noch einen weiteren Satz (z. B. 0 % für bestimmte Fälle)?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

---

## E · Texte auf Rechnung und E-Mail

### TXT-01 · Anrede

Die Anrede richtet sich nach dem Feld *Anrede* der Kundin bzw. des Kunden:

| Anrede  | Text auf Rechnung und in der E-Mail |
| ------- | ----------------------------------- |
| Herr    | Sehr geehrter Herr Mustermann,       |
| Frau    | Sehr geehrte Frau Mustermann,        |
| Familie | Sehr geehrte Familie Mustermann,     |

Es wird **nur der Nachname** genannt, kein Vorname. Ihr altes Programm schrieb
*„Sehr geehrte(r) Herr…"* — das entfällt, weil k-vet die Anrede kennt.

**Frage:** Passt die Form, oder möchten Sie den Vornamen mit drin haben
(*„Sehr geehrter Herr Thomas Mustermann"*)?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### TXT-02 · Zwei Namen im Haushalt

Sind zwei Personen hinterlegt, lautet die Anrede:
*„Sehr geehrte Frau Müller, sehr geehrter Herr Müller,"*

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### TXT-03 · Zahlungstext und Frist

Unter der Anrede steht:

> Vielen Dank für Ihr Vertrauen! Bitte überweisen Sie den Rechnungsbetrag bis zum 12.08.2026.

Die Frist beträgt **14 Tage** ab Rechnungsdatum. Zusätzlich steht oben bei den Rechnungsdaten eine
eigene Zeile **„Fälligkeitsdatum: 12.08.2026"** — die ist dafür da, dass lexoffice das Datum beim
Hochladen selbst erkennt und Sie es nicht mehr von Hand eintragen müssen (siehe `OFF-01`).

**Frage:** Sind 14 Tage richtig, und passt der Text?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### TXT-04 · Fußzeile

Auf **jeder** Seite steht unten:

> Mobile Tierärztin Dr. Erika Musterfrau · Musterstraße 1, 12345 Musterstadt · praxis@example.com
> Musterbank · IBAN: DE02 1203 0000 0000 2020 51 · BIC: BYLADEM1001
> USt-IdNr: DE123456789

Die Angaben oben sind Platzhalter — gedruckt werden die Werte aus *Einstellungen*.

Gegenüber Ihrer alten Rechnung fehlt die Zeile **„Geschäftsführung: Dr. Erika Musterfrau"** — die
hatten wir als verzichtbar eingestuft.

**Frage:** Soll die Zeile wieder rein? Und stimmen Bankname, IBAN, BIC und USt-IdNr.?

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

### TXT-05 · Betreff der E-Mail

> Ihre Rechnung RE-289 – Mobile Tierärztin Dr. Erika Musterfrau

*Status:* ☐ bestätigt  ☐ Rückfrage: ______________________________________________

---

## F · Offene Punkte

### OFF-01 · Test mit lexoffice

Bitte laden Sie **eine** mit k-vet erstellte Rechnung wie gewohnt in lexoffice hoch und schauen
Sie nach, ob das **Fälligkeitsdatum automatisch ausgefüllt** ist.

* **Ja** → erledigt, Sie müssen es nicht mehr eintippen.
* **Nein** → dann bauen wir zusätzlich eine maschinenlesbare Datei (XRechnung), die lexoffice
  vollständig einliest. Das ist etwas Arbeit, deshalb testen wir erst.

*Ergebnis:* ☐ Datum war ausgefüllt  ☐ Datum fehlte weiterhin

### OFF-02 · Kundennummer

Ihre alte Rechnung zeigt oben eine **„Kunden-Nr: 5041524"**. In k-vet lassen wir diese Zeile weg,
weil die Rechnungsnummer die Rechnung eindeutig identifiziert.

**Frage:** Vermissen Sie die Kundennummer? Falls ja: sollen wir die alten Nummern übernehmen
können, oder genügt eine neue laufende Nummer?

*Status:* ☐ ohne Kundennummer ist in Ordnung  ☐ Rückfrage: ______________________

---

## Abschluss

Wenn alle Punkte abgehakt sind, tragen Sie bitte hier ein:

* Geprüft von: ______________________________
* Datum: ______________________________

Diese Prüfung ist der Nachweis, dass die Preisberechnung fachlich abgenommen wurde
(Erfolgskriterium SC-005 der Spezifikation). Sie sollte wiederholt werden, wenn sich die GOT, die
AMPreisV oder der Mehrwertsteuersatz ändern.
