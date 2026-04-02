// audio.js — Web Audio API feedback tones for scan events
// Four programmatically generated sounds, zero external files.
(function () {
    "use strict";

    var ctx = null;

    function getContext() {
        if (!ctx) {
            try {
                ctx = new (window.AudioContext || window.webkitAudioContext)();
            } catch (e) {
                return null;
            }
        }
        return ctx;
    }

    function playTone(freq, type, duration) {
        var ac = getContext();
        if (!ac) return;
        var osc = ac.createOscillator();
        var gain = ac.createGain();
        osc.type = type;
        osc.frequency.setValueAtTime(freq, ac.currentTime);
        gain.gain.setValueAtTime(0.3, ac.currentTime);
        osc.connect(gain);
        gain.connect(ac.destination);
        osc.start(ac.currentTime);
        osc.stop(ac.currentTime + duration);
    }

    var AudioFeedback = {
        // Success: 880Hz sine, 80ms
        playSuccess: function () {
            playTone(880, "sine", 0.08);
        },

        // Info: 660Hz sine, 100ms
        playInfo: function () {
            playTone(660, "sine", 0.1);
        },

        // Warning: 440Hz square, 60ms x 2 with 40ms gap
        playWarning: function () {
            var ac = getContext();
            if (!ac) return;
            var gain = ac.createGain();
            gain.gain.setValueAtTime(0.3, ac.currentTime);
            gain.connect(ac.destination);

            var osc1 = ac.createOscillator();
            osc1.type = "square";
            osc1.frequency.setValueAtTime(440, ac.currentTime);
            osc1.connect(gain);
            osc1.start(ac.currentTime);
            osc1.stop(ac.currentTime + 0.06);

            var osc2 = ac.createOscillator();
            osc2.type = "square";
            osc2.frequency.setValueAtTime(440, ac.currentTime);
            osc2.connect(gain);
            osc2.start(ac.currentTime + 0.1); // 60ms + 40ms gap
            osc2.stop(ac.currentTime + 0.16);
        },

        // Error: 330Hz->220Hz sawtooth sweep, 150ms
        playError: function () {
            var ac = getContext();
            if (!ac) return;
            var osc = ac.createOscillator();
            var gain = ac.createGain();
            osc.type = "sawtooth";
            osc.frequency.setValueAtTime(330, ac.currentTime);
            osc.frequency.linearRampToValueAtTime(220, ac.currentTime + 0.15);
            gain.gain.setValueAtTime(0.3, ac.currentTime);
            osc.connect(gain);
            gain.connect(ac.destination);
            osc.start(ac.currentTime);
            osc.stop(ac.currentTime + 0.15);
        },

        isEnabled: function () {
            return localStorage.getItem("mybibli_audio_enabled") === "true";
        },

        toggle: function () {
            var enabled = !this.isEnabled();
            localStorage.setItem("mybibli_audio_enabled", String(enabled));
            this.updateToggleUI(enabled);
            return enabled;
        },

        updateToggleUI: function (enabled) {
            var btn = document.getElementById("audio-toggle");
            if (!btn) return;
            var iconOn = btn.querySelector(".icon-audio-on");
            var iconOff = btn.querySelector(".icon-audio-off");
            if (iconOn) iconOn.style.display = enabled ? "block" : "none";
            if (iconOff) iconOff.style.display = enabled ? "none" : "block";
            btn.setAttribute("aria-label", enabled ? (btn.dataset.labelDisable || "Disable scan sounds") : (btn.dataset.labelEnable || "Enable scan sounds"));
        },

        initToggle: function () {
            this.updateToggleUI(this.isEnabled());
        }
    };

    window.mybibliAudio = AudioFeedback;
})();
