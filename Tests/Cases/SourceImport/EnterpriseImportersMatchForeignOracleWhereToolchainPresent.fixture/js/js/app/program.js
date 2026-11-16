/**
 * @param {number} left
 * @param {number} right
 * @returns {number}
 */
function add(left, right) {
    return left + right;
}

/** @returns {void} */
function run() {
    console.log(add(2, 3));
}

/**
 * @param {object} values
 * @returns {number}
 */
function unsupported(values) {
    return values.length;
}
