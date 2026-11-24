// Hours the rock likely needs to dry, by precipitation in a rolling window (inches).
// The first row whose limit the amount is under applies.
const DRYING_HOURS = [
  { underInches: 0.25, hours: 24 },
  { underInches: 1, hours: 48 },
  { underInches: Infinity, hours: 72 },
];
// Melting snow keeps wetting the rock, so add this when at least half of a window's precipitation was snow.
const SNOW_EXTRA_HOURS = 24;

// The API's "YYYY-MM-DD HH:MM:SS" is already Utah (Mountain) local time, so format
// its parts as-is rather than letting the browser shift it to the viewer's zone.
function formatMountain(dateTime) {
  const [y, mo, d, h, mi] = dateTime.split(/[- :]/).map(Number);
  const month = new Date(Date.UTC(y, mo - 1, 1)).toLocaleString("en-US", { month: "short", timeZone: "UTC" });
  const hour12 = h % 12 || 12;
  return `${month} ${d}, ${hour12}:${String(mi).padStart(2, "0")} ${h < 12 ? "AM" : "PM"}`;
}

// Previews and `wrangler dev` send the Worker's debug log back in X-Debug-Log (see src/lib.rs).
function logDebug(r) {
  const log = r.headers.get("X-Debug-Log");
  if (log) JSON.parse(log).forEach((entry) => console.debug(new URL(r.url).pathname, entry));
  return r;
}

fetch("/api/precipitation")
  .then(logDebug)
  .then((r) => { if (!r.ok) throw new Error(r.status); return r.json(); })
  .then((data) => {
    Chart.defaults.color = "#fff"; // axis labels and legend
    Chart.defaults.borderColor = "#333"; // grid lines
    new Chart(document.getElementById("chart"), {
      type: "bar",
      data: {
        // Array labels render on two lines: weekday above, M/D below.
        labels: data.days.map((d) => [d.weekday, d.date.slice(5).split("-").map(Number).join("/")]),
        // Each day's bar stacks rain under snow (precip in hours averaging 34°F or colder; see src/lib.rs).
        // Responses cached before snow_in existed lack it, so treat it as 0 rather than blanking the chart.
        datasets: [
          // Chart.js's former auto color; setting any dataset's color turns its auto colors off.
          { label: "Rain (in)", data: data.days.map((d) => +(d.precip_in - (d.snow_in ?? 0)).toFixed(2)), backgroundColor: "rgba(54, 162, 235, 0.5)", borderColor: "rgb(54, 162, 235)" },
          { label: "Snow (in)", data: data.days.map((d) => d.snow_in), backgroundColor: "#f3faff" },
        ],
      },
      options: { scales: { x: { stacked: true }, y: { beginAtZero: true, stacked: true } } },
    });

    document.getElementById("recent").insertAdjacentHTML("beforeend", data.recent
      .map((r) => `<li><strong>Last ${r.hours} hours:</strong> ${r.precip_in == null ? "no data" : r.precip_in.toFixed(2) + " in"}</li>`)
      .join(""));

    const ago = (hours) => {
      const days = Math.floor(hours / 24);
      return hours < 1 ? "less than an hour ago"
        : days ? `${days} day${days === 1 ? "" : "s"} ago`
        : `${hours} hour${hours === 1 ? "" : "s"} ago`;
    };

    const last = data.last_precipitation;
    let lastText = "None in the last 2 weeks";
    if (last) {
      const text = ago(last.hours_ago);
      lastText = `${text[0].toUpperCase()}${text.slice(1)} (${formatMountain(last.date_time)})`;
    }

    // Drying time for each rolling window (24, 48, 72 hours; see DRYING_HOURS). A window only counts
    // while its drying time is at least its length, i.e. its precipitation may not have dried yet.
    // The longest such drying time wins; on a tie, the longer window (it holds all the relevant precip).
    const windows = data.recent.map((r) => {
      const precip = r.precip_in ?? 0;
      const snow = r.snow_in ?? 0;
      const dryHours = DRYING_HOURS.find((d) => precip < d.underInches).hours
        + (precip > 0 && snow >= precip / 2 ? SNOW_EXTRA_HOURS : 0);
      return { hours: r.hours, precip, snow, dryHours };
    });
    const day = windows.find((w) => w.hours === 24);
    // The 24-hour window always qualifies, since the shortest drying time is 24 hours.
    const waiting = windows.reduce((best, w) => (w.dryHours >= w.hours && w.dryHours >= best.dryHours ? w : best), day);
    const { dryHours } = waiting;
    // The rock is likely dry once the drying time has passed since the last precipitation.
    const sinceLast = last ? last.hours_ago : Infinity;
    const hoursLeft = dryHours - sinceLast;
    // Red for the first half of the drying time, yellow for the second, then green.
    const level = hoursLeft <= 0 ? "green" : sinceLast < dryHours / 2 ? "red" : "yellow";
    const verdictText = {
      red: "Likely not safe to climb.",
      yellow: "Potentially unsafe to climb.",
      green: "Likely safe to climb.",
    }[level];
    // Report the waiting window while it lasts; once it has passed, report the last 24 hours.
    const shown = hoursLeft > 0 ? waiting : day;
    let detail = `In the last ${shown.hours} hours, there has been ${(shown.precip - shown.snow).toFixed(2)} in of rain and ${shown.snow.toFixed(2)} in of snow.`;
    if (hoursLeft > 0) {
      detail += `\nThe next time it's likely safe to climb is ${hoursLeft} hour${hoursLeft === 1 ? "" : "s"} from now.`;
    }
    const safetyText = `${verdictText}\n${detail}\nUse caution and check the rock and conditions yourself before climbing.`;

    const lastEl = document.getElementById("last-precipitation");
    lastEl.textContent = lastText;

    const safetyEl = document.getElementById("safety");
    // Bold the first sentence (the verdict); the rest keeps its "\n" line breaks.
    const breakAt = safetyText.indexOf("\n");
    const verdict = document.createElement("strong");
    verdict.textContent = safetyText.slice(0, breakAt);
    safetyEl.replaceChildren(verdict, safetyText.slice(breakAt));
    safetyEl.className = `status ${level}`;
  })
  .catch((err) => {
    document.getElementById("safety").textContent = `Unknown: Precipitation data couldn't be loaded (${err.message}).`;
  });

fetch("/api/forecast")
  .then(logDebug)
  .then((r) => { if (!r.ok) throw new Error(r.status); return r.json(); })
  .then((data) => {
    const temp = (t) => (t == null ? "–" : `${t}°F`);
    document.getElementById("forecast-rows").replaceChildren(...data.days.map((d) => {
      const tr = document.createElement("tr");
      const day = document.createElement("th");
      day.scope = "row";
      day.textContent = d.name;
      const cells = [d.short, `${temp(d.high)} / ${temp(d.low)}`, `${d.precip_pct}%`, d.precip_in == null ? "–" : `${d.precip_in.toFixed(2)} in`].map((text) => {
        const td = document.createElement("td");
        td.textContent = text;
        return td;
      });
      tr.replaceChildren(day, ...cells);
      return tr;
    }));
    // Responses cached before "updated" existed lack it, so leave the issue time out.
    if (data.updated) {
      const issued = new Date(data.updated).toLocaleString("en-US", {
        timeZone: "America/Denver", month: "short", day: "numeric", hour: "numeric", minute: "2-digit",
      });
      document.getElementById("forecast-issued").textContent = `NOAA Forecast issued on ${issued} MT.`;
    }
  })
  .catch((err) => {
    document.getElementById("forecast-rows").innerHTML = `<tr><td colspan="5">Couldn't load the forecast (${err.message}).</td></tr>`;
  });
