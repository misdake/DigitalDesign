import {Editor} from "./Editor";
import {GameComp} from "./GameComp";
import {System} from "../logic/System";
import {registerBasicComponents} from "../logic/components/basic";
import {GameWire} from "./GameWire";
import {EventHost} from "../util/EventHost";

export class Game extends EventHost {
    readonly system: System;

    readonly templates: GameComp[];
    readonly components: GameComp[];
    readonly wires: GameWire[];

    readonly editor: Editor;

    constructor() {
        super();

        this.system = new System();
        registerBasicComponents(this.system);

        this.templates = [];
        this.components = [];
        this.wires = [];

        this.editor = new Editor(this);
    }
}