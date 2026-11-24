package demo;

import demo.greet.Greeter;

class Main {
    /** 😀 */ void tick() {}

    void ping(int a) {}

    void ping(String a) {}

    void run() {
        String s = "😀😀"; Greeter greeter = new Greeter(); greeter.wave(); tick(); ping(s);
    }
}
