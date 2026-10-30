import assert from 'node:assert/strict';
import {test} from 'node:test';
import {deadlineLabel} from '../src/dashboard-utils.mjs';
test('Le tableau de bord affiche le titre du travail sans son préfixe interne',()=>{
 assert.equal(deadlineLabel("work:Changer la chasse d’eau"),"Changer la chasse d’eau");
 assert.equal(deadlineLabel('Contrôle électrique'),'Contrôle électrique');
 assert.equal(deadlineLabel('work:Intervention work: locale'),'Intervention work: locale');
});
