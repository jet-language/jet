counter <- 0

transform <- function(input) {
  counter <<- counter + 1
  list(count = counter, nested = input$nested, vector = input$vector, scalar = input$scalar, nothing = NULL)
}

double_values <- function(input) input * 2

scale_table <- function(input) {
  if (!is.data.frame(input)) stop("typed table did not arrive as an R data.frame")
  input$score <- input$score * 2
  input
}

plot_scores <- function(input) {
  values <- unlist(input$values, use.names = FALSE)
  plot(values, type = "b", main = "Jet and R", xlab = "sample", ylab = "score")
}

fail_call <- function(input) stop("raw secret failure detail")
sleep_call <- function(input) { Sys.sleep(30); input }
