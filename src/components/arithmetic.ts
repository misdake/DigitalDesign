import {System} from "../System";
import {PinTemplate, PinType, WireTemplate} from "../ComponentTemplate";

export function registerArithmeticComponents(system: System) {
    system.registerCustomComponent({
            type: "full-adder",
            inputPins: [

                {name: "A", width: 1, type: PinType.BOOL},
                {name: "B", width: 1, type: PinType.BOOL},
                {name: "Cin", width: 1, type: PinType.BOOL},

            ] as PinTemplate[],
            components: [

                {name: "xor1", type: "xor"},
                {name: "xor2", type: "xor"},
                {name: "and1", type: "and"},
                {name: "and2", type: "and"},
                {name: "or", type: "or"},

            ] as ({ name: string, type: string })[],
            outputPins: [

                {name: "S", width: 1, type: PinType.BOOL},
                {name: "Cout", width: 1, type: PinType.BOOL},

            ] as PinTemplate[],

            wires: [

                //低位
                WireTemplate.create("this.A", "xor1.in0"),
                WireTemplate.create("this.B", "xor1.in1"),
                WireTemplate.create("xor1.out", "xor2.in0"),
                WireTemplate.create("this.Cin", "xor2.in1"),
                WireTemplate.create("xor2.out", "this.S"),

                //进位
                WireTemplate.create("this.A", "and1.in0"),
                WireTemplate.create("this.B", "and1.in1"),
                WireTemplate.create("xor1.out", "and2.in0"),
                WireTemplate.create("this.Cin", "and2.in1"),
                WireTemplate.create("and1.out", "or.in0"),
                WireTemplate.create("and2.out", "or.in1"),
                WireTemplate.create("or.out", "this.Cout"),

            ] as WireTemplate[],
    });

    system.registerCustomComponent({
        type: "4bit-adder",
        inputPins: [

            {name: "A", width: 4, type: PinType.UNSIGNED},
            {name: "B", width: 4, type: PinType.UNSIGNED},
            {name: "Cin", width: 1, type: PinType.BOOL},

        ] as PinTemplate[],
        components: [

            {name: "unpackA", type: "unpack4"},
            {name: "unpackB", type: "unpack4"},
            {name: "fad0", type: "full-adder"},
            {name: "fad1", type: "full-adder"},
            {name: "fad2", type: "full-adder"},
            {name: "fad3", type: "full-adder"},
            {name: "packS", type: "pack4"},

        ] as ({ name: string, type: string })[],
        outputPins: [

            {name: "S", width: 4, type: PinType.UNSIGNED},
            {name: "Cout", width: 1, type: PinType.BOOL},

        ] as PinTemplate[],

        wires: [

            WireTemplate.create("this.A", "unpackA.in"),
            WireTemplate.create("this.B", "unpackB.in"),

            WireTemplate.create("unpackA.out0", "fad0.A"),
            WireTemplate.create("unpackA.out1", "fad1.A"),
            WireTemplate.create("unpackA.out2", "fad2.A"),
            WireTemplate.create("unpackA.out3", "fad3.A"),
            WireTemplate.create("unpackB.out0", "fad0.B"),
            WireTemplate.create("unpackB.out1", "fad1.B"),
            WireTemplate.create("unpackB.out2", "fad2.B"),
            WireTemplate.create("unpackB.out3", "fad3.B"),

            WireTemplate.create("this.Cin", "fad0.Cin"),
            WireTemplate.create("fad0.Cout", "fad1.Cin"),
            WireTemplate.create("fad1.Cout", "fad2.Cin"),
            WireTemplate.create("fad2.Cout", "fad3.Cin"),
            WireTemplate.create("fad3.Cout", "this.Cout"),

            WireTemplate.create("fad0.S", "packS.in0"),
            WireTemplate.create("fad1.S", "packS.in1"),
            WireTemplate.create("fad2.S", "packS.in2"),
            WireTemplate.create("fad3.S", "packS.in3"),
            WireTemplate.create("packS.out", "this.S"),

        ] as WireTemplate[],
    });
}