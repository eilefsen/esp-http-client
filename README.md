This is a fairly basic HTTP client for the ESP32.
The microcontroller drives a sh1106 OLED display.


![esp_http_client](https://github.com/eilefsen/esp-http-client/assets/95104378/daed6bce-5b4a-40e5-a14e-7f7e31aa7e61)


The purpose is to monitor local bus departures so i can check it before leaving the house, potentially saving me a few minutes out in the cold.

The API i'm targeting is specific to Norway, although it is based on a larger project likely used elsewhere.

The crates for esp are not super well documented (imo), so this took a bit of guesswork to implement.

PS: yes, the display is in fact just taped to the board lol
