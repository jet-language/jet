counter <- 0

transform <- function(input) {
  counter <<- counter + 1
  list(count = counter, nested = input$nested, vector = input$vector, scalar = input$scalar, nothing = NULL)
}

double_values <- function(input) input * 2

fail_call <- function(input) stop("raw secret failure detail")
sleep_call <- function(input) { Sys.sleep(30); input }
