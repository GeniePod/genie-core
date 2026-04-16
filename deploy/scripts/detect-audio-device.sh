#!/bin/bash
# Detect the USB audio device (Lenovo E03 or any USB audio).
# Outputs the ALSA device string (e.g., "plughw:0,0" or "plughw:2,0").
# Used by genie-core to auto-configure audio_device.

# Look for USB audio in ALSA cards
CARD=$(cat /proc/asound/cards 2>/dev/null | grep -i "USB-Audio\|USB Audio\|Lenovo\|Headphone\|Headset" | head -1 | awk '{print $1}')

if [ -n "$CARD" ]; then
    echo "plughw:${CARD},0"
    exit 0
fi

# Fallback: try card 0
if [ -e /proc/asound/card0 ]; then
    echo "plughw:0,0"
    exit 0
fi

# No audio device found
echo ""
exit 1
