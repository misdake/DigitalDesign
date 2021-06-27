import {Game} from "../Game";
import {Editor} from "../Editor";
import {GameComp, GameCompPack, GameCompTemplate} from "../GameComp";
import {Events} from "../../util/Events";

export class EditorComponent {
    private game: Game;
    private editor: Editor;
    constructor(game: Game, editor: Editor) {
        this.game = game;
        this.editor = editor;
    }


    private nextCompId = 1;

    createTemplateComponent(template: GameCompTemplate, x: number, y: number): GameComp {
        let pack = new GameCompPack(template, x, y);
        let comp = new GameComp(this.nextCompId++, this.game.system, pack);
        this.game.templates.push(comp);
        this.game.fire(Events.COMPONENT_ADD, comp);
        return comp;
    }

    createRealComponent(templateComp: GameComp) {
        if (!templateComp.isTemplate) return;
        let index = this.game.templates.indexOf(templateComp);
        if (index >= 0) {
            templateComp.isTemplate = false;
            this.game.templates.splice(index, 1);
            this.createTemplateComponent(templateComp, templateComp.x, templateComp.y);
            this.game.components.push(templateComp);

            this.game.fire(Events.COMPONENT_ADD, templateComp);
        }
    }

    removeRealComponent(gameComp: GameComp): boolean {
        const index = this.game.components.indexOf(gameComp);
        if (index > -1) {
            //TODO 在这里删除相关的gameWire？
            this.game.components.splice(index, 1);
            this.game.fire(Events.COMPONENT_REMOVE, gameComp);
            return true;
        }
        return false;
    }

}