package com.acme.probe;

import java.util.Map;

/** `Map.Entry` is java.util.Map.Entry: the default-package Map is not visible here. */
class UsesEntry {
    Map.Entry<String, String> e;
}
