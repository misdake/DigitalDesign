import {Game} from "../Game";
import {Editor} from "../Editor";
import {GameComp, GameCompTemplate} from "../GameComp";

export class EditorComponent {
    private game: Game;
    private editor: Editor;
    constructor(game: Game, editor: Editor) {
        this.game = game;
        this.editor = editor;
    }


    private nextCompId = 1;

    createComponent(template: GameCompTemplate, x: number, y: number): GameComp {
        let pack = {...template, x: x, y: y};
        let comp = new GameComp(this.nextCompId++, this.game.system, pack);
        this.game.components.push(comp);
        this.editor.doUpdate(); //TODO 支持一次添加多个
        return comp;
    }

    moveComponent(gameComp: GameComp, x: number, y: number) {
        gameComp.x = x;
        gameComp.y = y;
        console.log("editor moveComponent", gameComp.name, x, y);
        gameComp.fire("move", x, y);
    }

    removeComponent(gameComp: GameComp): boolean {
        const index = this.game.components.indexOf(gameComp);
        if (index > -1) {

            //TODO 在这里删除相关的gameWire？

            this.game.components.splice(index, 1);

            this.editor.doUpdate();

            return true;
        }
        return false;
    }

}