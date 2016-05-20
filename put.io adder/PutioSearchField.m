//
//  PutioSearchField.m
//  put.io adder
//
//  Created by Nico on 02/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import "PutioSearchField.h"
#import "PutioSearchResultsTableView.h"
#import "PutioHelper.h"
#import "AFNetworking.h"
#import "NSString+UrlEncode.h"
#import "PutioResultsViewController.h"

@implementation PutioSearchField

- (void)drawRect:(NSRect)dirtyRect {
    [super drawRect:dirtyRect];
    
    // Drawing code here.
//    self.placeholderString = @"Search";
}


- (void)textDidChange:(NSNotification *)notification
{
    [super textDidChange:notification];
    
    PutioHelper *helper = [PutioHelper sharedHelper];
//    PutioSearchResultsTableView *results = helper.putioController.searchResults;
    
   // return;
//    [helper.putioController.popResults.]
    
    NSString *query = [self.stringValue stringByTrimmingCharactersInSet:
                        [NSCharacterSet whitespaceCharacterSet]];
    
    if (query.length > 3)
    {
//        results.superview.superview.frame = CGRectMake(
//            self.frame.origin.x,
//            self.frame.origin.y - 80,
//            self.frame.size.width,
//            80
//        );

//        [results.superview.superview setHidden:NO];
//
//        NSView *superview = [results superview];
//        [results removeFromSuperview];
//        [superview addSubview:results];
        
//        NSProgressIndicator *indicator = [[NSProgressIndicator alloc] initWithFrame:CGRectMake(
//            self.frame.size.width / 2 - 20,
//            10,
//            20,
//            20
//        )];

//        indicator.style = NSProgressIndicatorSpinningStyle;
//        [indicator startAnimation:nil];
//        [results.superview addSubview:indicator];
        
        
        NSString *baseurl = @"https://api.put.io/";
        NSString *path = [NSString stringWithFormat:@"/v2/files/search/%@/page/1?oauth_token=%@", [query urlEncode], helper.putioAPI.apiToken];

        AFHTTPClient *client = [[AFHTTPClient alloc] initWithBaseURL:[NSURL URLWithString:baseurl]];
        [client registerHTTPOperationClass:[AFJSONRequestOperation class]];
        [client setDefaultHeader:@"Accept" value:@"application/json"];
        [client setParameterEncoding:AFJSONParameterEncoding];
        
        [client
         getPath:path
         parameters: nil
         success: ^(AFHTTPRequestOperation *operation, id JSON)
         {
             PutioResultsViewController *table = (PutioResultsViewController*) helper.putioController.popResults.contentViewController;
             
             table.files = [JSON objectForKey:@"files"];
             NSLog(@"res: %lu", (unsigned long)table.files.count); 
            
             if (table.files.count > 0)
             {
                
                 
                 // table.tableView.superview == NSPopoverFrame
//                 NSTableView *tab = table.tableView;
                 
//                 PutioSearchResultsTableView *tab = helper.putioController.searchResults;
//                 tab.frame = CGRectMake(table.tableView.superview.frame.origin.x, table.tableView.superview.frame.origin.y, 180, 250);
//                 
                 [table.tableView reloadData];
                 [helper.putioController.popResults showRelativeToRect:self.bounds ofView:self preferredEdge:NSMaxXEdge];
                 
                 CGSize size = CGSizeMake(300, table.files.count * 20);
                 [helper.putioController.popResults setContentSize:size];
//                 [helper.putioController.popResults

//                 NSLog(@"super %@", tab.superview.class);
//                 [helper.putioController.searchResults setFrame:CGRectMake(table.tableView.superview.frame.origin.x, table.tableView.superview.frame.origin.y, 180, 250)];
                 
//                 [table.tableView.superview setFrame:CGRectMake(table.tableView.superview.frame.origin.x, table.tableView.superview.frame.origin.y, 180, 250)];
//                 [table.tableView.scrollView.documentView setFrame: NSMakeRect(0,0,new_width, new_height) ];
             }
             
             [self becomeFirstResponder];
         }
         failure:^(AFHTTPRequestOperation *operation, NSError *error)
         {
              NSLog(@"Failed to fetch screenshot/icon");
         }];

    }
    else
    {
//        [results.superview.superview setHidden:YES];
        [helper.putioController.popResults performClose:nil];
    }
}

@end
