class PinData {
    name: string;
    width: number;
    // type: ??;
}

class WireData {
    name: string;
    fromName: string;
    fromPin: string;
    toName: string;
    toPin: string;
}

class ComponentData {
    type: string;
    inputPins: PinData[];
    components: { name: string, type: string }[];
    outputPins: PinData[];
    wire: WireData[];
}