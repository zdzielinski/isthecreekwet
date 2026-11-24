# Is The Creek Wet?

**[isthecreekwet.com](https://isthecreekwet.com)** helps climbers decide whether the sandstone at Indian Creek, Utah, is dry enough to climb.

Wet sandstone is weak. Climbing on it too soon after rain or snow can break holds and loosen anchors, so the BLM asks climbers to wait at least 24–48 hours after precipitation. This site puts the information you need to make that call on one page:

- **Recent Precipitation:** when it last rained or snowed, and an hourly chart of the last 14 days from the Dugout Ranch weather station.
- **Forecast:** the National Weather Service forecast for the area, including expected precipitation.
- **Sunny Crags:** crags that get the most sun, for drying out on cold or damp days.

## Data Sources

- **Hourly Station Data:** the [Dugout Ranch weather station](https://climate.usu.edu/mchd/dashboard/dashboard.php?network=AGWX&station=1349823&units=E&showgraph=0&) (AGWX network, station 1349823), provided by the [Utah Climate Center](https://climate.usu.edu/) at Utah State University through its [API](https://logansw.atlassian.net/wiki/spaces/UCCA/overview).
- **Forecast:** the [National Weather Service forecast](https://forecast.weather.gov/MapClick.php?lat=38.104&lon=-109.595) (NOAA) for the station's location (38.104, -109.595), from the [NWS API](https://www.weather.gov/documentation/services-web-api).

## How It's Built

The site runs on [Cloudflare Workers](https://workers.cloudflare.com/). The backend is Rust compiled to WebAssembly, and the frontend is plain HTML, CSS and JavaScript.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and local development.

## License

Released under the [MIT License](LICENSE.md).
