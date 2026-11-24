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

    const last = data.last_precipitation;
    let lastText = "None in the last 2 weeks";
    if (last) {
      const days = Math.floor(last.hours_ago / 24);
      const hours = last.hours_ago;
      const ago = hours < 1 ? "Less than an hour ago"
        : days ? `${days} day${days === 1 ? "" : "s"} ago`
        : `${hours} hour${hours === 1 ? "" : "s"} ago`;
      lastText = `${ago} (${formatMountain(last.date_time)})`;
    }
    // Safety guideline: precipitation < 24h ago is red, < 48h orange, < 72h yellow, anything older is green.
    const h = last ? last.hours_ago : Infinity;
    const level = h < 24 ? "red" : h < 48 ? "orange" : h < 72 ? "yellow" : "green";
    const safetyText = {
      red: "Likely not safe to climb.\nPrecipitation was detected less than 24 hours ago.\nUse caution and check the rock and conditions yourself before climbing.",
      orange: "Likely unsafe to climb.\nPrecipitation was detected less than 48 hours ago.\nUse caution and check the rock and conditions yourself before climbing.",
      yellow: "Potentially unsafe to climb.\nPrecipitation was detected less than 72 hours ago.\nUse caution and check the rock and conditions yourself before climbing.",
      green: "Likely safe to climb.\nNo precipitation has been detected in the last 72 hours.\nUse caution and check the rock and conditions yourself before climbing.",
    }[level];

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
  })
  .catch((err) => {
    document.getElementById("forecast-rows").innerHTML = `<tr><td colspan="5">Couldn't load the forecast (${err.message}).</td></tr>`;
  });
