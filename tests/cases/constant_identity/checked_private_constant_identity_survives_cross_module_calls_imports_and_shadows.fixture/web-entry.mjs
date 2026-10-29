const { jet_main } = await import('./app.js');
const result = await jet_main();
if (result !== undefined) throw new Error('web entry success result must be undefined');