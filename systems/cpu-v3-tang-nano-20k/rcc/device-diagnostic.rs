fn main() {
    dev_send(0, 2, 0b010101);
    while 1 == 1 {
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x44);
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x44);
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x48);
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x54);
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x01);
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x09);
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x00);
        while dev_recv(0, 3) & 1 != 0 {}
        dev_send(0, 3, 0x14);
    }
}
