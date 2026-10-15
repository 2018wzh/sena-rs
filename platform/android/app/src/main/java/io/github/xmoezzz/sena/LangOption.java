package io.github.xmoezzz.sena;

public enum LangOption {
    CN("简体中文", "cn"),
    TC("繁體中文", "tc"),
    JP("日本語", "jp");

    public static final LangOption DEFAULT = LangOption.CN;

    public final String label;
    public final String value;

    LangOption(String label, String value) {
        this.label = label;
        this.value = value;
    }

    public static LangOption fromValue(String value) {
        if (value != null) {
            for (LangOption option : values()) {
                if (option.value.equalsIgnoreCase(value) || option.label.equalsIgnoreCase(value)) {
                    return option;
                }
            }
        }
        return DEFAULT;
    }

    public static String[] labels() {
        LangOption[] options = values();
        String[] labels = new String[options.length];
        for (int i = 0; i < options.length; i++) {
            labels[i] = options[i].label;
        }
        return labels;
    }

    public static int indexOfValue(String value) {
        LangOption option = fromValue(value);
        LangOption[] options = values();
        for (int i = 0; i < options.length; i++) {
            if (options[i] == option) return i;
        }
        return 0;
    }
}
